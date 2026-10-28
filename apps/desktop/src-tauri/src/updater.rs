//! Self-updates: the port of `App/Updater.swift` (ARCHITECTURE.md "Updates").
//! `tauri-plugin-updater` reads `latest.json` from the latest GitHub release
//! (`plugins.updater` in tauri.conf.json holds the feed and the minisign
//! public key), downloads the installer it names, checks the installer's
//! signature and runs it. That installer is the kind the app was installed
//! with (NSIS or MSI), so the app and its `ctx.exe` sidecar always move
//! together, as the whole bundle does with Sparkle. The schedule is
//! Sparkle's: a check shortly after launch once a day has passed since the
//! last one, then daily; an update it finds downloads in the background and
//! installs when the app quits, or, with automatic installs off, is offered
//! through `updater:found`. The settings Sparkle keeps in its user defaults
//! live in `<app config dir>/updater.json`. `scripts/release.ps1` signs the
//! installers and writes the feed.
//!
//! Only copies the bundler packaged into an installer update themselves
//! (`Updates::available`): `tauri dev` and `cargo build` binaries, `--mock`
//! and `--autotest` runs, and the macOS and Linux builds (which have no feed)
//! never reach the network. `CTX_UPDATE_FEED=<url>` points an installed copy
//! at another feed (a staging release, a local test server); updates must
//! still carry a valid signature.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::utils::config::BundleType;
use tauri::{AppHandle, Emitter, Manager, State, Url};
use tauri_plugin_updater::{Update, UpdaterExt};
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

use crate::backend::{lock, Backends};
use crate::selftest::LaunchOptions;

/// Sparkle's default `SUScheduledCheckInterval`: one day, in seconds.
const CHECK_INTERVAL_SECS: i64 = 24 * 60 * 60;
/// The first scheduled check waits this long after launch, out of startup's way.
const LAUNCH_DELAY: Duration = Duration::from_secs(15);
/// How often the schedule looks at the clock; checks stay a day apart.
const SCHEDULE_TICK: Duration = Duration::from_secs(60 * 60);
/// Limit for the feed request (not the download: installers are large).
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// Download progress goes to the frontend at most this often.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

const UNAVAILABLE: &str = "This build does not update itself.";

/// Sparkle's user defaults (`SUEnableAutomaticChecks`,
/// `SUAutomaticallyUpdate`, `SULastCheckTime`, `SUSkippedVersion`), stored as
/// `updater.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    automatically_checks: bool,
    automatically_installs: bool,
    /// Unix seconds of the last finished check.
    last_check: Option<i64>,
    /// Skip This Version: scheduled checks pass over this release.
    skipped_version: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { automatically_checks: true, automatically_installs: true, last_check: None, skipped_version: None }
    }
}

/// A release the feed announced: `{ version, notes, date }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Release {
    pub version: String,
    /// The release notes (Markdown), when the feed has them.
    pub notes: Option<String>,
    /// RFC 3339 publication date.
    pub date: Option<String>,
}

impl Release {
    fn of(update: &Update) -> Self {
        let date = update.date.and_then(|date| DateTime::from_timestamp(date.unix_timestamp(), 0)).map(|date| date.to_rfc3339());
        let notes = update.body.clone().filter(|notes| !notes.trim().is_empty());
        Self { version: update.version.clone(), notes, date }
    }
}

/// Where the updater is: `{ kind: "idle" | "checking" | "available" | "downloading" | "ready", … }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Checking,
    /// Found and not downloaded; Install Update downloads it.
    Available { release: Release },
    Downloading { release: Release, received: u64, total: Option<u64> },
    /// Downloaded and verified: installs when the app quits, or at once with Install and Relaunch.
    Ready { release: Release },
}

/// `updater_status()` and the `updater:state` event.
#[derive(Debug, Clone, Serialize)]
pub struct UpdaterStatus {
    /// `Updater.isAvailable`: false for builds that never update themselves.
    pub available: bool,
    /// The running version (release.ps1 sets it to the release version).
    pub version: String,
    pub automatically_checks: bool,
    pub automatically_installs: bool,
    /// RFC 3339 time of the last check; `null` before the first.
    pub last_check: Option<String>,
    pub phase: Phase,
}

/// The updater, as managed state (`AppState.updater` in Swift).
pub struct Updates {
    available: bool,
    version: String,
    settings_path: Option<PathBuf>,
    inner: Mutex<Inner>,
    /// One check at a time: Check Now during a scheduled check waits for it, then checks again.
    checking: tokio::sync::Mutex<()>,
    /// Wakes the schedule when the settings change.
    wake: tokio::sync::Notify,
}

struct Inner {
    settings: Settings,
    phase: Phase,
    /// The update `phase` refers to (available, downloading or ready).
    update: Option<Update>,
    /// Its verified installer, once downloaded.
    bytes: Option<Vec<u8>>,
}

impl Updates {
    pub fn new(app: &AppHandle) -> Self {
        let options = LaunchOptions::current();
        let available = cfg!(windows) && !options.mock && options.autotest.is_none() && installed();
        let settings_path = app.path().app_config_dir().ok().map(|dir| dir.join("updater.json"));
        let settings = match &settings_path {
            Some(path) if available => read_settings(path),
            _ => Settings::default(),
        };
        Self {
            available,
            version: app.package_info().version.to_string(),
            settings_path,
            inner: Mutex::new(Inner { settings, phase: Phase::Idle, update: None, bytes: None }),
            checking: tokio::sync::Mutex::new(()),
            wake: tokio::sync::Notify::new(),
        }
    }

    pub fn status(&self) -> UpdaterStatus {
        let inner = lock(&self.inner);
        UpdaterStatus {
            available: self.available,
            version: self.version.clone(),
            automatically_checks: inner.settings.automatically_checks,
            automatically_installs: inner.settings.automatically_installs,
            last_check: inner.settings.last_check.and_then(|seconds| DateTime::from_timestamp(seconds, 0)).map(|date| date.to_rfc3339()),
            phase: inner.phase.clone(),
        }
    }

    /// Sparkle's scheduled checks: shortly after launch once a day has
    /// passed since the last check, then daily while automatic checks are on.
    pub fn start_schedule(app: AppHandle) {
        tauri::async_runtime::spawn(async move {
            let updates = app.state::<Updates>();
            if !updates.available {
                return;
            }
            tokio::time::sleep(LAUNCH_DELAY).await;
            loop {
                if updates.check_is_due() {
                    updates.scheduled_check(&app).await;
                }
                tokio::select! {
                    _ = tokio::time::sleep(SCHEDULE_TICK) => {}
                    _ = updates.wake.notified() => {}
                }
            }
        });
    }

    fn check_is_due(&self) -> bool {
        let inner = lock(&self.inner);
        let settings = &inner.settings;
        settings.automatically_checks
            && inner.phase == Phase::Idle
            && settings.last_check.is_none_or(|last| Utc::now().timestamp() - last >= CHECK_INTERVAL_SECS)
    }

    async fn scheduled_check(&self, app: &AppHandle) {
        match self.check(app, false).await {
            Ok(status) => {
                let Phase::Available { release } = status.phase else { return };
                if !status.automatically_installs {
                    let _ = app.emit("updater:found", release);
                } else if let Err(error) = self.download(app).await {
                    eprintln!("[updater] downloading {} failed: {error}", release.version);
                }
            }
            Err(error) => eprintln!("[updater] scheduled check failed: {error}"),
        }
    }

    /// One look at the feed. A check the user asked for also offers a
    /// skipped version, as Sparkle's does. A downloaded update stays as it is.
    pub async fn check(&self, app: &AppHandle, user: bool) -> Result<UpdaterStatus, String> {
        if !self.available {
            return Err(UNAVAILABLE.into());
        }
        let _one_at_a_time = self.checking.lock().await;
        {
            let mut inner = lock(&self.inner);
            if matches!(inner.phase, Phase::Downloading { .. } | Phase::Ready { .. }) {
                drop(inner);
                return Ok(self.status());
            }
            inner.phase = Phase::Checking;
        }
        self.publish(app);
        let result = match self.updater(app) {
            Ok(updater) => updater.check().await.map_err(|error| error.to_string()),
            Err(error) => Err(error),
        };
        {
            let mut inner = lock(&self.inner);
            let inner = &mut *inner;
            inner.settings.last_check = Some(Utc::now().timestamp());
            match &result {
                Ok(Some(update)) if user || inner.settings.skipped_version.as_deref() != Some(update.version.as_str()) => {
                    inner.phase = Phase::Available { release: Release::of(update) };
                    inner.update = Some(update.clone());
                }
                _ => {
                    inner.phase = Phase::Idle;
                    inner.update = None;
                }
            }
            self.save(&inner.settings);
        }
        self.publish(app);
        result.map(|_| self.status())
    }

    /// Install Update, and the background download: fetches the installer of
    /// the update found last and checks its signature.
    pub async fn download(&self, app: &AppHandle) -> Result<UpdaterStatus, String> {
        if !self.available {
            return Err(UNAVAILABLE.into());
        }
        let (update, release) = {
            let mut inner = lock(&self.inner);
            let (Phase::Available { release }, Some(update)) = (&inner.phase, &inner.update) else {
                return Err("There is no update to download. Check for updates first.".into());
            };
            let (update, release) = (update.clone(), release.clone());
            inner.phase = Phase::Downloading { release: release.clone(), received: 0, total: None };
            (update, release)
        };
        self.publish(app);
        let mut received = 0u64;
        let mut reported = Instant::now();
        let result = update
            .download(
                |chunk, total| {
                    received += chunk as u64;
                    if reported.elapsed() >= PROGRESS_INTERVAL {
                        reported = Instant::now();
                        lock(&self.inner).phase = Phase::Downloading { release: release.clone(), received, total };
                        self.publish(app);
                    }
                },
                || {},
            )
            .await;
        let failure = {
            let mut inner = lock(&self.inner);
            match result {
                Ok(bytes) => {
                    inner.bytes = Some(bytes);
                    inner.phase = Phase::Ready { release };
                    None
                }
                Err(error) => {
                    inner.phase = Phase::Available { release };
                    Some(error.to_string())
                }
            }
        };
        self.publish(app);
        match failure {
            Some(message) => Err(message),
            None => Ok(self.status()),
        }
    }

    /// Install and Relaunch: stops every backend (the local ctx and each
    /// host's ssh) so the installer can replace `ctx.exe`, then starts the
    /// installer, which opens the app again when it is done. On success the
    /// process exits here; an error leaves the backends stopped (the
    /// frontend starts them again).
    pub async fn install_now(&self, app: &AppHandle) -> Result<(), String> {
        if !self.available {
            return Err(UNAVAILABLE.into());
        }
        let (update, bytes) = self.take_downloaded().ok_or("No update has been downloaded yet.")?;
        for backend in app.state::<Backends>().all() {
            backend.stop().await;
        }
        let _ = app.save_window_state(StateFlags::all());
        let installed = update.install(&bytes).map_err(|error| error.to_string());
        self.put_back(update, bytes);
        installed
    }

    /// `RunEvent::Exit`, once the backend has stopped: a downloaded update
    /// installs now and the app stays closed (Sparkle's install on quit).
    /// `install` exits the process, which lets the installer replace it.
    pub fn install_on_quit(&self) {
        let Some((update, bytes)) = self.take_downloaded() else { return };
        let version = update.version.clone();
        if let Err(error) = update.restart_after_install(false).install(&bytes) {
            eprintln!("[updater] could not install {version} on quit: {error}");
        }
    }

    /// Skip This Version (`skip`) or Remind Me Later: forgets the update that
    /// was found. A downloaded one is kept.
    pub fn dismiss(&self, app: &AppHandle, skip: bool) -> Result<UpdaterStatus, String> {
        if !self.available {
            return Err(UNAVAILABLE.into());
        }
        {
            let mut inner = lock(&self.inner);
            let inner = &mut *inner;
            if let Phase::Available { release } = &inner.phase {
                if skip {
                    inner.settings.skipped_version = Some(release.version.clone());
                    self.save(&inner.settings);
                }
                inner.phase = Phase::Idle;
                inner.update = None;
            }
        }
        self.publish(app);
        Ok(self.status())
    }

    /// Settings → Updates → Automatic.
    pub fn set_automatic(&self, app: &AppHandle, checks: Option<bool>, installs: Option<bool>) -> Result<UpdaterStatus, String> {
        if !self.available {
            return Err(UNAVAILABLE.into());
        }
        {
            let mut inner = lock(&self.inner);
            if let Some(checks) = checks {
                inner.settings.automatically_checks = checks;
            }
            if let Some(installs) = installs {
                inner.settings.automatically_installs = installs;
            }
            self.save(&inner.settings);
        }
        self.wake.notify_one();
        self.publish(app);
        Ok(self.status())
    }

    fn updater(&self, app: &AppHandle) -> Result<tauri_plugin_updater::Updater, String> {
        let mut builder = app.updater_builder().timeout(CHECK_TIMEOUT);
        if let Some(feed) = std::env::var("CTX_UPDATE_FEED").ok().filter(|feed| !feed.is_empty()) {
            let url: Url = feed.parse().map_err(|error| format!("CTX_UPDATE_FEED is not a URL: {error}"))?;
            builder = builder.endpoints(vec![url]).map_err(|error| error.to_string())?;
        }
        builder.build().map_err(|error| error.to_string())
    }

    fn take_downloaded(&self) -> Option<(Update, Vec<u8>)> {
        let mut inner = lock(&self.inner);
        if !matches!(inner.phase, Phase::Ready { .. }) {
            return None;
        }
        Some((inner.update.clone()?, inner.bytes.take()?))
    }

    fn put_back(&self, update: Update, bytes: Vec<u8>) {
        let mut inner = lock(&self.inner);
        inner.update = Some(update);
        inner.bytes = Some(bytes);
    }

    fn publish(&self, app: &AppHandle) {
        let _ = app.emit("updater:state", self.status());
    }

    fn save(&self, settings: &Settings) {
        let Some(path) = &self.settings_path else { return };
        let written = serde_json::to_vec_pretty(settings)
            .map_err(std::io::Error::other)
            .and_then(|json| path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(path, json)));
        if let Err(error) = written {
            eprintln!("[updater] could not save {}: {error}", path.display());
        }
    }
}

/// The bundler marked this binary as the app inside an NSIS or MSI
/// installer; `tauri dev` and `cargo build` binaries carry no mark.
fn installed() -> bool {
    matches!(tauri::utils::platform::bundle_type(), Some(BundleType::Nsis | BundleType::Msi))
}

fn read_settings(path: &Path) -> Settings {
    std::fs::read(path).ok().and_then(|json| serde_json::from_slice(&json).ok()).unwrap_or_default()
}

// MARK: - Commands

/// `updater_status()` → `UpdaterStatus`.
#[tauri::command]
pub fn updater_status(updates: State<'_, Updates>) -> Result<UpdaterStatus, String> {
    Ok(updates.status())
}

/// `updater_check()`: Check for Updates… / Check Now.
#[tauri::command]
pub async fn updater_check(app: AppHandle, updates: State<'_, Updates>) -> Result<UpdaterStatus, String> {
    updates.check(&app, true).await
}

/// `updater_download()`: Install Update; resolves once the installer is downloaded and verified.
#[tauri::command]
pub async fn updater_download(app: AppHandle, updates: State<'_, Updates>) -> Result<UpdaterStatus, String> {
    updates.download(&app).await
}

/// `updater_install()`: Install and Relaunch. Resolves only with an error.
#[tauri::command]
pub async fn updater_install(app: AppHandle, updates: State<'_, Updates>) -> Result<(), String> {
    updates.install_now(&app).await
}

/// `updater_dismiss({ skip })`: Skip This Version / Remind Me Later.
#[tauri::command]
pub fn updater_dismiss(app: AppHandle, updates: State<'_, Updates>, skip: bool) -> Result<UpdaterStatus, String> {
    updates.dismiss(&app, skip)
}

/// `updater_set_automatic({ checks?, installs? })`.
#[tauri::command]
pub fn updater_set_automatic(app: AppHandle, updates: State<'_, Updates>, checks: Option<bool>, installs: Option<bool>) -> Result<UpdaterStatus, String> {
    updates.set_automatic(&app, checks, installs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_to_sparkles_defaults() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings, Settings::default());
        assert!(settings.automatically_checks && settings.automatically_installs);
        let settings: Settings = serde_json::from_str(r#"{ "automatically_installs": false, "last_check": 1700000000 }"#).unwrap();
        assert!(settings.automatically_checks);
        assert!(!settings.automatically_installs);
        assert_eq!(settings.last_check, Some(1_700_000_000));
    }

    #[test]
    fn phases_serialize_as_the_frontend_reads_them() {
        let release = Release { version: "0.1.3".into(), notes: None, date: None };
        assert_eq!(serde_json::to_value(Phase::Idle).unwrap(), serde_json::json!({ "kind": "idle" }));
        assert_eq!(
            serde_json::to_value(Phase::Downloading { release, received: 10, total: Some(20) }).unwrap(),
            serde_json::json!({ "kind": "downloading", "release": { "version": "0.1.3", "notes": null, "date": null }, "received": 10, "total": 20 })
        );
    }
}
