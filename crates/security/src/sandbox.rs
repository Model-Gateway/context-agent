//! Developer-mode command wrapping (spec §11.2). There is no user-facing
//! sandbox mode any more: `[permissions.sandbox]` is for tests and
//! developers who want every shell command run through a wrapper. The
//! status describes what is actually in effect, never implied protection,
//! and the policy engine never grants anything because a wrapper is set.

use ctx_core::{SandboxConfig, SandboxKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxStatus {
    pub kind: SandboxKind,
    /// True only when commands run under an enforced isolation boundary.
    pub enforced: bool,
    pub description: String,
    pub wrapper: Vec<String>,
}

impl SandboxStatus {
    pub fn from_config(cfg: &SandboxConfig) -> Self {
        match cfg.kind {
            SandboxKind::None => Self {
                kind: SandboxKind::None,
                enforced: false,
                description: "No OS-level sandbox is configured. Path checks apply to built-in file tools only; shell commands run with the user's full privileges and can access any resource. Approval policies compensate for this.".into(),
                wrapper: Vec::new(),
            },
            SandboxKind::Wrapper => {
                if cfg.wrapper.is_empty() {
                    Self {
                        kind: SandboxKind::None,
                        enforced: false,
                        description: "Sandbox kind is `wrapper` but no wrapper command is configured; treating as no sandbox.".into(),
                        wrapper: Vec::new(),
                    }
                } else {
                    Self {
                        kind: SandboxKind::Wrapper,
                        enforced: true,
                        description: format!(
                            "Shell commands are prefixed with the configured wrapper `{}`. Isolation strength depends entirely on that wrapper.",
                            cfg.wrapper.join(" ")
                        ),
                        wrapper: cfg.wrapper.clone(),
                    }
                }
            }
        }
    }

    /// Wrap a shell command line into the program and arguments that run it
    /// in `shell` (see [`ctx_core::platform::Shell`] for how each kind of
    /// shell takes a command), honouring the developer wrapper.
    pub fn wrap_command(&self, shell: &str, command: &str) -> WrappedCommand {
        let shell = ctx_core::platform::Shell::from_program(shell);
        let invocation = shell.invocation(command);
        if self.enforced && !self.wrapper.is_empty() {
            let mut args: Vec<String> = self.wrapper[1..].to_vec();
            args.push(shell.program.clone());
            args.extend(invocation.args);
            WrappedCommand { program: self.wrapper[0].clone(), args, raw_tail: invocation.raw_tail }
        } else {
            WrappedCommand { program: shell.program, args: invocation.args, raw_tail: invocation.raw_tail }
        }
    }
}

/// A command line ready to spawn: `program` with `args`, then (Windows,
/// `cmd.exe` only) `raw_tail` appended verbatim to the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrappedCommand {
    pub program: String,
    pub args: Vec<String>,
    pub raw_tail: Option<String>,
}
