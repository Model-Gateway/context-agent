//! Linked git worktrees for parallel sessions (`worktree/create`,
//! `worktree/list`, `worktree/remove`, `worktree/merge`).
//!
//! `task/run` allows one run per project root, so a second task in the same
//! repository would otherwise queue. A worktree is a second checkout of the
//! same repository on its own branch: opened as a project of its own it has
//! its own run slot, index and chats, and the two never edit the same files.
//! Worktrees live under `<data_dir>/worktrees/<parent id>/<slug>` and start
//! from a copy of the parent's index store, so the first indexing pass only
//! hashes files instead of parsing and embedding them again.

use super::dto::*;
use super::gitops::{git_failure, git_line, head_commit};
use super::support::*;
use super::{blocking, OpenParams, Project, RpcError, RpcResult, Server};
use ctx_core::{CtxError, RepositoryId};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const ADJECTIVES: &[&str] = &["amber", "brisk", "calm", "clear", "crisp", "eager", "fresh", "gentle", "keen", "light", "lively", "lucid", "mellow", "neat", "quick", "quiet", "sharp", "steady", "swift", "tidy"];
const NOUNS: &[&str] = &["aspen", "brook", "cedar", "cliff", "cove", "dune", "fern", "field", "glade", "harbor", "heath", "inlet", "knoll", "marsh", "meadow", "ridge", "shore", "spring", "summit", "willow"];

/// The main worktree's root when `root` is a linked worktree, else `None`.
/// Synchronous: `project_info` calls it from `spawn_blocking`.
pub(super) fn parent_root(root: &Path) -> Option<PathBuf> {
    let run = |args: &[&str]| -> Option<String> {
        let out = ctx_core::platform::hide_console_window(&mut std::process::Command::new("git")).args(args).current_dir(root).env("GIT_OPTIONAL_LOCKS", "0").output().ok()?;
        if !out.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let git_dir = run(&["rev-parse", "--path-format=absolute", "--git-dir"])?;
    let common = run(&["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    if git_dir == common {
        return None;
    }
    let common = PathBuf::from(common);
    let parent = common.parent()?.to_path_buf();
    Some(ctx_core::platform::canonicalize(&parent).unwrap_or(parent))
}

/// Whether `path` is one `worktree/create` made (under the data dir).
pub(super) fn is_managed(layout_worktrees_dir: &Path, path: &Path) -> bool {
    let dir = ctx_core::platform::canonicalize(layout_worktrees_dir).unwrap_or_else(|_| layout_worktrees_dir.to_path_buf());
    let path = ctx_core::platform::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    path.starts_with(dir)
}

/// Lowercase ASCII letters, digits and single dashes; at most 40 bytes.
pub(super) fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
        if out.len() >= 40 {
            break;
        }
    }
    out.trim_end_matches('-').to_string()
}

/// `brisk-cedar`: picked from the clock, no RNG dependency needed.
fn random_slug() -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let a = ADJECTIVES[(nanos % ADJECTIVES.len() as u128) as usize];
    let n = NOUNS[((nanos / 7919) % NOUNS.len() as u128) as usize];
    format!("{a}-{n}")
}

/// Parse `git worktree list --porcelain`.
pub(super) fn parse_worktree_list(text: &str, worktrees_dir: &Path) -> Vec<WorktreeEntry> {
    let mut out = Vec::new();
    for (i, block) in text.split("\n\n").filter(|b| !b.trim().is_empty()).enumerate() {
        let mut path = None;
        let mut head = None;
        let mut branch = None;
        for line in block.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                path = Some(p.trim().to_string());
            } else if let Some(h) = line.strip_prefix("HEAD ") {
                head = Some(h.trim().to_string());
            } else if let Some(b) = line.strip_prefix("branch ") {
                branch = Some(b.trim().trim_start_matches("refs/heads/").to_string());
            }
        }
        if let Some(path) = path {
            let managed = is_managed(worktrees_dir, Path::new(&path));
            out.push(WorktreeEntry { path, branch, head, is_main: i == 0, managed });
        }
    }
    out
}

impl Server {
    fn worktrees_dir(&self) -> PathBuf {
        self.config.read().layout().worktrees_dir()
    }

    /// The main root of a project: itself, or its parent for a worktree.
    async fn main_root(&self, project: &Arc<Project>) -> PathBuf {
        let root = project.ctx.root.clone();
        let probe = root.clone();
        blocking(move || Ok(parent_root(&probe))).await.ok().flatten().unwrap_or(root)
    }

    /// `worktree/create`: adds `<data_dir>/worktrees/<parent id>/<slug>` on a
    /// new branch `ctx/<slug>`, seeds its index from the parent's, opens it
    /// as a project and returns that project's `ProjectInfo`.
    pub(super) async fn worktree_create(self: &Arc<Self>, params: WorktreeCreateParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let main = self.main_root(&project).await;
        let probe = main.clone();
        let inside_repository = blocking(move || Ok(ctx_indexing::RepositoryDiscovery::git_info(&probe).is_some())).await?;
        if !inside_repository {
            return Err(RpcError::invalid_params(format!("{} is not a git repository", main.display())));
        }
        let parent_id = RepositoryId::from_root(&main);
        let base_slug = params.name.as_deref().map(slugify).filter(|s| !s.is_empty()).unwrap_or_else(random_slug);
        // A free folder and a free branch name: `-2`, `-3`, … otherwise.
        let mut slug = base_slug.clone();
        let mut n = 1;
        let path = loop {
            let candidate = self.config.read().layout().worktree_dir(&parent_id, &slug);
            let branch_taken = git(&main, &["rev-parse", "--verify", "-q", &format!("refs/heads/ctx/{slug}")]).await.map(|o| o.status.success()).unwrap_or(false);
            if !candidate.exists() && !branch_taken {
                break candidate;
            }
            n += 1;
            if n > 50 {
                return Err(RpcError::from(CtxError::other(format!("no free worktree name for {base_slug}"))));
            }
            slug = format!("{base_slug}-{n}");
        };
        let branch = format!("ctx/{slug}");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(CtxError::from)?;
        }
        let base = params.base.as_deref().filter(|b| !b.trim().is_empty()).unwrap_or("HEAD");
        if base.starts_with('-') {
            return Err(RpcError::invalid_params(format!("invalid base: {base}")));
        }
        let path_text = path.to_string_lossy().to_string();
        let out = git(&main, &["worktree", "add", "-b", &branch, &path_text, base]).await.map_err(CtxError::from)?;
        if !out.status.success() {
            return Err(RpcError::from(CtxError::other(format!("git worktree add failed: {}", git_error_line(&out.stderr)))));
        }
        // Seed the index: a copy of the parent's store under the new id. Best
        // effort — without it the worktree is indexed from scratch.
        let new_id = RepositoryId::from_root(&path);
        let dest = self.config.read().layout().index_db(&new_id);
        if !dest.exists() {
            let store = project.ctx.stores.index.clone();
            let copy_dest = dest.clone();
            if let Err(e) = blocking(move || store.copy_to(&copy_dest)).await {
                self.log("warn", format!("could not seed the worktree index from {}: {}", main.display(), e.message));
                let _ = std::fs::remove_file(&dest);
            }
        }
        self.project_open(OpenParams { path: path_text, exact: true, init_git: false }).await
    }

    /// `worktree/list`: every worktree of the project's repository.
    pub(super) async fn worktree_list(&self, params: ProjectParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let main = self.main_root(&project).await;
        let out = git(&main, &["worktree", "list", "--porcelain"]).await.map_err(CtxError::from)?;
        if !out.status.success() {
            return Err(RpcError::from(CtxError::other(format!("git worktree list failed: {}", git_error_line(&out.stderr)))));
        }
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let entries = parse_worktree_list(&text, &self.worktrees_dir());
        Ok(json!({"worktrees": entries}))
    }

    /// `worktree/remove`: closes the worktree's project, removes the
    /// checkout and, on request, its branch. Refused while a task runs there.
    pub(super) async fn worktree_remove(&self, params: WorktreeRemoveParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let root = project.ctx.root.clone();
        let probe = root.clone();
        let Some(main) = blocking(move || Ok(parent_root(&probe))).await? else {
            return Err(RpcError::invalid_params(format!("{} is not a linked worktree", root.display())));
        };
        if project.is_running() {
            return Err(RpcError::task_running());
        }
        let branch = ctx_indexing::RepositoryDiscovery::git_info(&root).and_then(|g| g.branch);
        self.project_close(ProjectParams { project_id: params.project_id.clone() })?;
        let path_text = root.to_string_lossy().to_string();
        let mut args = vec!["worktree", "remove"];
        if params.force {
            args.push("--force");
        }
        args.push(&path_text);
        let out = git(&main, &args).await.map_err(CtxError::from)?;
        if !out.status.success() {
            return Err(RpcError::from(CtxError::other(format!("git worktree remove failed: {}", git_error_line(&out.stderr)))));
        }
        let mut branch_deleted = false;
        if params.delete_branch {
            if let Some(branch) = branch.as_deref() {
                let out = git(&main, &["branch", "-D", branch]).await.map_err(CtxError::from)?;
                if out.status.success() {
                    branch_deleted = true;
                } else {
                    self.log("warn", format!("could not delete branch {branch}: {}", git_error_line(&out.stderr)));
                }
            }
        }
        Ok(json!({"removed": path_text, "branch": branch, "branch_deleted": branch_deleted}))
    }

    /// The open project whose root is `root` (the parent of a worktree).
    fn project_by_root(&self, root: &Path) -> Option<Arc<Project>> {
        let want = ctx_core::platform::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        self.projects.read().values().find(|p| ctx_core::platform::canonicalize(&p.ctx.root).unwrap_or_else(|_| p.ctx.root.clone()) == want).cloned()
    }

    /// `worktree/merge`: `git merge --no-ff --no-edit ctx/<slug>` in the main
    /// checkout, then (on request) `worktree/remove` with the branch. Refused
    /// while either project runs, with uncommitted changes in the worktree,
    /// and on a conflict (the merge is aborted and the paths reported).
    pub(super) async fn worktree_merge(&self, params: WorktreeMergeParams) -> RpcResult<Value> {
        let project = self.project(&params.project_id)?;
        let root = project.ctx.root.clone();
        let probe = root.clone();
        let Some(main) = blocking(move || Ok(parent_root(&probe))).await? else {
            return Err(RpcError::invalid_params(format!("{} is not a linked worktree", root.display())));
        };
        if project.is_running() {
            return Err(RpcError::task_running());
        }
        if let Some(parent) = self.project_by_root(&main) {
            if parent.is_running() {
                return Err(RpcError::task_running());
            }
        }
        let branch = git_line(&root, &["symbolic-ref", "--short", "-q", "HEAD"]).await.ok_or_else(|| RpcError::from(CtxError::other("the worktree is not on a branch")))?;
        let target = git_line(&main, &["symbolic-ref", "--short", "-q", "HEAD"]).await.ok_or_else(|| RpcError::from(CtxError::other(format!("{} is not on a branch, so there is nothing to merge into", main.display()))))?;
        if branch == target {
            return Err(RpcError::from(CtxError::other(format!("{branch} is checked out in the parent as well"))));
        }
        let status = git(&root, &["status", "--porcelain=v1", "-z", "--untracked-files=all"]).await.map_err(CtxError::from)?;
        if !status.status.success() {
            return Err(git_failure("git status", &status));
        }
        if !status.stdout.is_empty() {
            return Err(RpcError::from(CtxError::other(format!("the worktree has uncommitted changes: commit them before merging {branch}"))));
        }
        let ahead = git_line(&main, &["rev-list", "--count", &format!("{target}..{branch}")]).await.unwrap_or_default();
        if ahead == "0" {
            return Err(RpcError::from(CtxError::other(format!("nothing to merge: {target} already has every commit of {branch}"))));
        }
        let out = git(&main, &["merge", "--no-ff", "--no-edit", &branch]).await.map_err(CtxError::from)?;
        if !out.status.success() {
            let conflicts = git(&main, &["diff", "--name-only", "--diff-filter=U"]).await.map(|o| String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect::<Vec<_>>()).unwrap_or_default();
            // A merge git started (MERGE_HEAD exists) is taken back; one it
            // refused up front (a dirty parent tree) left nothing behind.
            if git_line(&main, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]).await.is_some() {
                let abort = git(&main, &["merge", "--abort"]).await.map_err(CtxError::from)?;
                if !abort.status.success() {
                    self.log("warn", format!("git merge --abort failed in {}: {}", main.display(), git_error_line(&abort.stderr)));
                }
            }
            if !conflicts.is_empty() {
                return Err(RpcError::from(CtxError::other(format!("merging {branch} into {target} conflicts in {}; the merge was aborted", conflicts.join(", ")))));
            }
            return Err(git_failure("git merge", &out));
        }
        let merged = head_commit(&main).await?;
        let mut removed = false;
        if params.delete_after {
            if let Err(e) = self.worktree_remove(WorktreeRemoveParams { project_id: params.project_id.clone(), delete_branch: true, force: false }).await {
                return Err(RpcError::from(CtxError::other(format!("merged {branch} into {target} as {}, but removing the worktree failed: {}", &merged.commit[..merged.commit.len().min(8)], e.message))));
            }
            removed = true;
        }
        Ok(json!({"merged": merged.commit, "subject": merged.subject, "removed": removed}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_safe_folder_and_branch_names() {
        assert_eq!(slugify("Fix the Login Bug!"), "fix-the-login-bug");
        assert_eq!(slugify("  --weird__name  "), "weird-name");
        assert_eq!(slugify("***"), "");
        assert!(slugify(&"x".repeat(100)).len() <= 40);
        let s = random_slug();
        assert!(s.contains('-') && s == slugify(&s), "{s}");
    }

    #[test]
    fn worktree_list_is_parsed() {
        let text = "worktree /repo\nHEAD aaaa\nbranch refs/heads/main\n\nworktree /data/worktrees/id/brisk-cedar\nHEAD bbbb\nbranch refs/heads/ctx/brisk-cedar\n\nworktree /else\nHEAD cccc\ndetached\n";
        let list = parse_worktree_list(text, Path::new("/data/worktrees"));
        assert_eq!(list.len(), 3);
        assert!(list[0].is_main && list[0].branch.as_deref() == Some("main") && !list[0].managed);
        assert!(!list[1].is_main && list[1].managed && list[1].branch.as_deref() == Some("ctx/brisk-cedar"));
        assert!(list[2].branch.is_none() && list[2].head.as_deref() == Some("cccc"));
    }
}
