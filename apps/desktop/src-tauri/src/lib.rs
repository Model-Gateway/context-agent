//! Context desktop (Tauri): the Rust shell. Plugins, managed state and the
//! command registry of ARCHITECTURE.md "Rust ↔ frontend". The modules mirror
//! the macOS app's `Backend/` folder and `App/LaunchOptions.swift`:
//!
//! - `backend`: the `ctx serve` child (`BackendProcess`, `RPCConnection`,
//!   `BackendLocator`, `BackendEnvironment`) behind the `backend_*` and
//!   `rpc_request` commands and the `backend:*` events.
//! - `secrets`: the OS credential store (`KeychainStore`).
//! - `fs`: the file tree, attachment reads, pasted images, the folder picker
//!   and the reveal / open actions.
//! - `selftest`: `LaunchOptions` (`--mock`, `--project`, `--binary`, `--autotest`)
//!   and `selftest_finish` (writes the self-test's `result.json`, then exits).
//! - `updater`: self-updates (`App/Updater.swift`) behind the `updater_*`
//!   commands and the `updater:*` events.
//!
//! Every command returns `Result<T, String>` with a user-presentable error
//! string (`backend_start` and `rpc_request` reject with JSON, see `backend`).

pub mod backend;
pub mod fs;
pub mod secrets;
pub mod selftest;
pub mod updater;
// F6: the Inspector's Terminal and Browser tabs.
pub mod browser;
pub mod terminal;

use std::time::Duration;

use tauri::{Manager, RunEvent};

/// How long the shell waits for the child to leave when the app quits.
const QUIT_STOP_TIMEOUT: Duration = Duration::from_secs(3);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(backend::Backend::default())
        // F6 registers its managed state here:
        .manage(terminal::Terminals::default())
        .manage(browser::Browsers::default())
        .setup(|app| {
            app.manage(updater::Updates::new(app.handle()));
            updater::Updates::start_schedule(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            selftest::launch_options,
            selftest::selftest_finish,
            backend::backend_start,
            backend::backend_stop,
            backend::backend_state,
            backend::backend_stderr,
            backend::rpc_request,
            backend::backend_candidates,
            secrets::secret_get,
            secrets::secret_set,
            secrets::secret_remove,
            fs::fs_list_dir,
            fs::fs_read_text,
            fs::fs_file_info,
            fs::fs_write_temp_image,
            fs::fs_save_text,
            fs::fs_pick_project,
            fs::reveal_in_file_manager,
            fs::open_with_default_app,
            fs::open_url,
            updater::updater_status,
            updater::updater_check,
            updater::updater_download,
            updater::updater_install,
            updater::updater_dismiss,
            updater::updater_set_automatic,
            // F6 registers its commands here:
            terminal::terminal_open,
            terminal::terminal_write,
            terminal::terminal_resize,
            terminal::terminal_close,
            browser::browser_create,
            browser::browser_state,
            browser::browser_navigate,
            browser::browser_back,
            browser::browser_forward,
            browser::browser_reload,
            browser::browser_stop,
            browser::browser_set_bounds,
            browser::browser_show,
            browser::browser_hide,
            browser::browser_destroy,
            browser::browser_perform,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            // Like the macOS app on quit: ask the child to shut down (then
            // kill it) before the process goes away, so no `ctx serve` is
            // left behind, and the installer of a downloaded update can
            // replace ctx.exe.
            let backend = app.state::<backend::Backend>();
            tauri::async_runtime::block_on(async {
                let _ = tokio::time::timeout(QUIT_STOP_TIMEOUT, backend.stop()).await;
            });
            app.state::<updater::Updates>().install_on_quit();
        }
    });
}
