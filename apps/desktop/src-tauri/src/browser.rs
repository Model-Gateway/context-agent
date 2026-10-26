//! The Inspector Browser tab's pages (`BrowserWorkspace.swift`): one child
//! webview per project, laid over the tab's area of the main window, that
//! the user browses in and the agent drives through `browser/request`
//! (docs/desktop-protocol.md "The browser tool"). The page outlives tab
//! switches: when another Inspector tab is selected it is moved out of the
//! window rather than hidden, so it keeps loading and running script at full
//! speed while the agent works in it (a hidden WebView2 or WKWebView is
//! throttled), and the agent's actions never change the selected tab.
//!
//! Tauri reports page loads and titles but no history, progress or errors,
//! so the page state of `BrowserWorkspace` (`url`, `title`, `isLoading`,
//! `progress`, `canGoBack`, `canGoForward`, `loadError`) is tracked here
//! from the `on_page_load`, `on_document_title_changed` and `on_navigation`
//! hooks plus a light poll of `location.href` for same-document navigations,
//! and pushed to the frontend as `browser:state` events.
//!
//! The agent's actions run the Swift app's scripts (numbering interactive
//! elements with `data-ctx-ref`, clicking, typing, selecting, scrolling,
//! evaluating a function body) with `Webview::eval_with_callback`. Scripts
//! may be asynchronous (`evaluate` allows `await`), so each one is started
//! under an id and its result is collected by polling the page: no IPC is
//! opened to remote pages, which therefore get no capability at all. An
//! initialization script installs the helper and auto-dismisses JavaScript
//! dialogs (the `WKUIDelegate` methods), recording them for the reply.

use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tauri::webview::{DownloadEvent, NewWindowResponse, PageLoadEvent, WebviewBuilder};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Rect, State, Url, Webview, WebviewUrl};

use crate::backend::{debug, lock};

/// Emitted whenever a page's state changes (`BrowserState`).
pub const STATE_EVENT: &str = "browser:state";
/// Child webview labels are `browser-<project id>-<n>`.
pub const LABEL_PREFIX: &str = "browser-";
const MAIN_WINDOW: &str = "main";

/// `BrowserWorkspace.startPage`.
const START_PAGE: &str = "about:blank";
/// `BrowserWorkspace.detachedSize`: the size a page renders at before the
/// tab first shows it.
const DETACHED_WIDTH: f64 = 1024.0;
const DETACHED_HEIGHT: f64 = 768.0;
/// How far left of the window a page that is not shown is parked.
const OFFSCREEN_MARGIN: f64 = 200.0;
const MAX_TEXT_CHARS: usize = 15_000;
const NAVIGATE_TEXT_CHARS: usize = 6_000;
const MAX_ELEMENTS: usize = 150;
/// `BrowserWorkspace.loadTimeout`.
const LOAD_TIMEOUT: Duration = Duration::from_secs(30);
/// `awaitLoad(graceMs:)`'s default and the click / submit grace.
const LOAD_GRACE: Duration = Duration::from_millis(300);
const HISTORY_GRACE: Duration = Duration::from_millis(500);
const ACTION_GRACE: Duration = Duration::from_millis(600);
/// "Let scripts that run on load settle before reading the DOM."
const SETTLE: Duration = Duration::from_millis(200);
/// One round trip into the page.
const EVAL_TIMEOUT: Duration = Duration::from_secs(10);
/// The agent's scripts; `evaluate` gets longer (the tool waits up to 90 s).
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(20);
const EVALUATE_TIMEOUT: Duration = Duration::from_secs(75);
/// How often a shown page is asked for its address (same-document navigations).
const LOCATION_POLL: Duration = Duration::from_secs(1);
const MAX_HISTORY: usize = 500;
const NO_PAGE: &str = "no page is open; use navigate first";
const GONE: &str = "the browser is gone";

// MARK: - Wire types

/// A page's area in the main window, in CSS pixels (logical points).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Bounds {
    const DETACHED: Bounds = Bounds { x: 0.0, y: 0.0, width: DETACHED_WIDTH, height: DETACHED_HEIGHT };

    fn sanitized(self) -> Bounds {
        let finite = |value: f64, fallback: f64| if value.is_finite() { value } else { fallback };
        Bounds {
            x: finite(self.x, 0.0),
            y: finite(self.y, 0.0),
            width: finite(self.width, DETACHED_WIDTH).max(1.0),
            height: finite(self.height, DETACHED_HEIGHT).max(1.0),
        }
    }
}

/// The `browser:state` payload and `browser_state`'s result: the observable
/// properties of `BrowserWorkspace`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BrowserState {
    pub project_id: String,
    /// The page's address; null before the first page (about:blank).
    pub url: Option<String>,
    pub title: String,
    pub loading: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    /// 0…1 while loading (requested, committed, finished).
    pub progress: f64,
    /// The last navigation failure, shown under the toolbar until the next load.
    pub load_error: Option<String>,
    /// Whether the page is placed in the window (`browser_show`).
    pub visible: bool,
}

/// `BrowserReply`: what `browser_perform` answers, sent on with `browser/respond`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BrowserReply {
    pub ok: bool,
    pub content: String,
    pub url: Option<String>,
    pub title: Option<String>,
    pub error: Option<String>,
}

// MARK: - Managed state

/// The pages, keyed by project id (`AppState.browsers`).
#[derive(Default)]
pub struct Browsers {
    pages: Mutex<HashMap<String, Arc<Page>>>,
    next_label: AtomicU64,
    create: tokio::sync::Mutex<()>,
}

impl Browsers {
    fn get(&self, project_id: &str) -> Option<Arc<Page>> {
        lock(&self.pages).get(project_id).cloned()
    }

    fn require(&self, project_id: &str) -> Result<Arc<Page>, String> {
        self.get(project_id).ok_or_else(|| "The project has no browser page.".to_string())
    }

    /// The project's page, created (blank, at `bounds` or parked out of the
    /// window at the detached size) on first use.
    async fn ensure(&self, app: &AppHandle, project_id: &str, bounds: Option<Bounds>, visible: bool) -> Result<Arc<Page>, String> {
        if let Some(page) = self.get(project_id) {
            return Ok(page);
        }
        let _creating = self.create.lock().await;
        if let Some(page) = self.get(project_id) {
            return Ok(page);
        }
        let window = app
            .get_window(MAIN_WINDOW)
            .ok_or_else(|| "The main window is not open.".to_string())?;
        let number = self.next_label.fetch_add(1, Ordering::Relaxed) + 1;
        let label = format!("{LABEL_PREFIX}{}-{number}", label_safe(project_id));
        let page = Arc::new(Page::new(app.clone(), project_id.to_string(), label, bounds.unwrap_or(Bounds::DETACHED), visible));
        let rect = page.placement();
        let webview = window
            .add_child(page.builder(), rect.position, rect.size)
            .map_err(|error| format!("Could not create the browser: {error}"))?;
        let _ = page.webview.set(webview);
        lock(&self.pages).insert(project_id.to_string(), page.clone());
        page.spawn_location_poll();
        page.emit_state();
        Ok(page)
    }

    fn remove(&self, project_id: &str) -> Option<Arc<Page>> {
        lock(&self.pages).remove(project_id)
    }
}

/// Tauri labels allow `a-zA-Z0-9-/:_`.
fn label_safe(text: &str) -> String {
    text.chars()
        .map(|character| if character.is_ascii_alphanumeric() || matches!(character, '-' | '/' | ':' | '_') { character } else { '_' })
        .collect()
}

// MARK: - Page

/// A history move the next commit belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Intent {
    Back,
    Forward,
    Reload,
}

/// What `awaitLoad` compares against: the counters and the native address
/// before the action.
#[derive(Debug, Clone)]
struct Marker {
    commits: u64,
    finishes: u64,
    starts: u64,
    native: Option<String>,
}

#[derive(Debug)]
struct PageState {
    url: Option<String>,
    title: String,
    loading: bool,
    progress: f64,
    load_error: Option<String>,
    /// Committed pages, oldest first; `index` is the current one.
    history: Vec<String>,
    index: usize,
    intent: Option<Intent>,
    /// Main-frame commits (`PageLoadEvent::Started`) and finishes.
    commits: u64,
    finishes: u64,
    /// Main-frame navigation requests (Windows only: elsewhere the hook also
    /// sees subframes).
    starts: u64,
    committed_since_finish: bool,
    /// Bumped by every navigation this module starts, so a stale watchdog stops.
    provisional: u64,
    bounds: Bounds,
    visible: bool,
}

impl PageState {
    fn can_go_back(&self) -> bool {
        !self.history.is_empty() && self.index > 0
    }

    fn can_go_forward(&self) -> bool {
        self.index + 1 < self.history.len()
    }

    /// Files a committed address into the history the way WebKit's back
    /// list would: a pending back / forward / reload moves the cursor, a new
    /// address drops the forward list.
    fn record(&mut self, url: &str) {
        match self.intent.take() {
            Some(Intent::Back) if self.index > 0 && !self.history.is_empty() => {
                self.index -= 1;
                self.history[self.index] = url.to_string();
            }
            Some(Intent::Forward) if self.can_go_forward() => {
                self.index += 1;
                self.history[self.index] = url.to_string();
            }
            Some(Intent::Reload) if !self.history.is_empty() => {
                self.history[self.index] = url.to_string();
            }
            _ => {
                if self.history.get(self.index).map(String::as_str) == Some(url) {
                    return;
                }
                if !self.history.is_empty() {
                    self.history.truncate(self.index + 1);
                }
                self.history.push(url.to_string());
                if self.history.len() > MAX_HISTORY {
                    self.history.remove(0);
                }
                self.index = self.history.len() - 1;
            }
        }
    }

    fn marker(&self, native: Option<String>) -> Marker {
        Marker { commits: self.commits, finishes: self.finishes, starts: self.starts, native }
    }
}

/// One project's page: the child webview and what is known about it.
struct Page {
    app: AppHandle,
    project_id: String,
    label: String,
    webview: OnceLock<Webview>,
    state: Mutex<PageState>,
    last_emitted: Mutex<Option<BrowserState>>,
    /// Serializes the agent's actions (two sessions may drive one browser).
    actions: tokio::sync::Mutex<()>,
    script_serial: AtomicU64,
    script_salt: u64,
    closed: AtomicBool,
}

impl Page {
    fn new(app: AppHandle, project_id: String, label: String, bounds: Bounds, visible: bool) -> Page {
        Page {
            app,
            project_id,
            label,
            webview: OnceLock::new(),
            state: Mutex::new(PageState {
                url: None,
                title: String::new(),
                loading: false,
                progress: 0.0,
                load_error: None,
                history: Vec::new(),
                index: 0,
                intent: None,
                commits: 0,
                finishes: 0,
                starts: 0,
                committed_since_finish: false,
                provisional: 0,
                bounds: bounds.sanitized(),
                visible,
            }),
            last_emitted: Mutex::new(None),
            actions: tokio::sync::Mutex::new(()),
            script_serial: AtomicU64::new(0),
            script_salt: RandomState::new().build_hasher().finish(),
            closed: AtomicBool::new(false),
        }
    }

    fn builder(self: &Arc<Self>) -> WebviewBuilder<tauri::Wry> {
        let start = Url::parse(START_PAGE).expect("about:blank is a URL");
        let on_load = Arc::downgrade(self);
        let on_title = Arc::downgrade(self);
        let on_navigation = Arc::downgrade(self);
        let on_new_window = Arc::downgrade(self);
        let navigation_app = self.app.clone();
        let window_app = self.app.clone();
        WebviewBuilder::new(&self.label, WebviewUrl::External(start))
            .initialization_script(BOOTSTRAP_SCRIPT)
            .zoom_hotkeys_enabled(true)
            .on_page_load(move |_webview, payload| {
                if let Some(page) = on_load.upgrade() {
                    match payload.event() {
                        PageLoadEvent::Started => page.page_started(payload.url()),
                        PageLoadEvent::Finished => page.page_finished(payload.url()),
                    }
                }
            })
            .on_document_title_changed(move |_webview, title| {
                if let Some(page) = on_title.upgrade() {
                    page.title_changed(title);
                }
            })
            .on_navigation(move |url| {
                let allowed = navigation_allowed(&navigation_app, url);
                // The blank start page may load before the webview is
                // registered, and then never reports its completion.
                if allowed && cfg!(windows) && is_web_url(url) {
                    if let Some(page) = on_navigation.upgrade() {
                        page.navigation_requested();
                    }
                }
                allowed
            })
            // Downloads are refused, as the Swift app's WKWebView (no
            // download delegate) refuses them.
            .on_download(|_webview, event| !matches!(event, DownloadEvent::Requested { .. }))
            .on_new_window(move |url, _features| {
                // Pop-ups (`target="_blank"`, `window.open`) open in the same view.
                if let Some(page) = on_new_window.upgrade() {
                    if is_web_url(&url) && navigation_allowed(&window_app, &url) {
                        tauri::async_runtime::spawn(async move {
                            if let Err(error) = page.load(url) {
                                debug(|| format!("browser: could not open a pop-up: {error}"));
                            }
                        });
                    }
                }
                NewWindowResponse::Deny
            })
    }

    fn webview(&self) -> Result<Webview, String> {
        if self.is_closed() {
            return Err(GONE.to_string());
        }
        self.webview.get().cloned().ok_or_else(|| GONE.to_string())
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    /// The webview's own idea of its address (provisional while a load is
    /// pending on WebKit).
    fn native_url(&self) -> Option<String> {
        let webview = self.webview().ok()?;
        webview.url().ok().map(|url| url.to_string())
    }

    fn snapshot(&self) -> BrowserState {
        let state = lock(&self.state);
        BrowserState {
            project_id: self.project_id.clone(),
            url: state.url.clone(),
            title: state.title.clone(),
            loading: state.loading,
            can_go_back: state.can_go_back(),
            can_go_forward: state.can_go_forward(),
            progress: state.progress,
            load_error: state.load_error.clone(),
            visible: state.visible,
        }
    }

    fn emit_state(&self) {
        if self.is_closed() {
            return;
        }
        let snapshot = self.snapshot();
        {
            let mut last = lock(&self.last_emitted);
            if last.as_ref() == Some(&snapshot) {
                return;
            }
            *last = Some(snapshot.clone());
        }
        if let Err(error) = self.app.emit(STATE_EVENT, snapshot) {
            debug(|| format!("browser: could not emit the page state: {error}"));
        }
    }

    fn has_page(&self) -> bool {
        lock(&self.state).url.is_some()
    }

    fn current(&self) -> (Option<String>, String) {
        let state = lock(&self.state);
        (state.url.clone(), state.title.clone())
    }

    fn marker(&self) -> Marker {
        let native = self.native_url();
        lock(&self.state).marker(native)
    }

    // MARK: Placement

    /// Where the webview goes: its bounds when shown, parked left of the
    /// window (same size) when not.
    fn placement(&self) -> Rect {
        let state = lock(&self.state);
        let bounds = state.bounds;
        let (x, y) = if state.visible { (bounds.x, bounds.y) } else { (-(bounds.width + OFFSCREEN_MARGIN), 0.0) };
        Rect {
            position: LogicalPosition::new(x, y).into(),
            size: LogicalSize::new(bounds.width, bounds.height).into(),
        }
    }

    fn place(&self) -> Result<(), String> {
        let webview = self.webview()?;
        webview
            .set_bounds(self.placement())
            .map_err(|error| format!("Could not place the browser: {error}"))
    }

    fn set_layout(&self, bounds: Option<Bounds>, visible: Option<bool>) -> Result<(), String> {
        {
            let mut state = lock(&self.state);
            if let Some(bounds) = bounds {
                state.bounds = bounds.sanitized();
            }
            if let Some(visible) = visible {
                state.visible = visible;
            }
        }
        self.place()?;
        self.emit_state();
        Ok(())
    }

    // MARK: Hooks

    fn page_started(&self, url: &Url) {
        {
            let mut state = lock(&self.state);
            let was_loading = state.loading;
            state.commits += 1;
            state.committed_since_finish = true;
            state.loading = true;
            state.progress = if was_loading { state.progress.max(0.6) } else { 0.6 };
            state.load_error = None;
            let address = page_address(url);
            if let Some(address) = &address {
                state.record(address);
            }
            state.url = address;
        }
        self.emit_state();
    }

    fn page_finished(self: &Arc<Self>, url: &Url) {
        {
            let mut state = lock(&self.state);
            state.finishes += 1;
            state.loading = false;
            state.progress = 1.0;
            let address = page_address(url);
            if !state.committed_since_finish {
                if let Some(address) = &address {
                    state.record(address);
                }
            }
            state.committed_since_finish = false;
            state.url = address;
        }
        self.emit_state();
        let page = self.clone();
        tauri::async_runtime::spawn(async move { page.refresh_location().await });
    }

    fn title_changed(self: &Arc<Self>, title: String) {
        {
            let mut state = lock(&self.state);
            if state.title == title {
                return;
            }
            state.title = title;
        }
        self.emit_state();
        // Single-page apps often retitle when they change the address.
        let page = self.clone();
        tauri::async_runtime::spawn(async move { page.refresh_location().await });
    }

    /// Windows' `NavigationStarting` (main frame only): the load is under way
    /// and `NavigationCompleted` will follow even when it fails.
    fn navigation_requested(self: &Arc<Self>) {
        lock(&self.state).starts += 1;
        self.begin_provisional(None, None);
    }

    /// Marks a load as requested (`isLoading` turns on at once, as WebKit
    /// does) and watches it: a load that never commits ends after
    /// `LOAD_TIMEOUT`, and on macOS one whose address falls back to the old
    /// page has failed (WebKit drops a failed provisional URL; Tauri reports
    /// no failure).
    fn begin_provisional(self: &Arc<Self>, target: Option<String>, native_before: Option<String>) {
        let (generation, commits) = {
            let mut state = lock(&self.state);
            state.provisional += 1;
            state.loading = true;
            state.progress = 0.1;
            state.load_error = None;
            (state.provisional, state.commits)
        };
        self.emit_state();
        let weak = Arc::downgrade(self);
        tauri::async_runtime::spawn(async move {
            let started = Instant::now();
            loop {
                tokio::time::sleep(Duration::from_millis(250)).await;
                let Some(page) = weak.upgrade() else { return };
                if page.is_closed() {
                    return;
                }
                {
                    let state = lock(&page.state);
                    if state.provisional != generation || state.commits != commits || !state.loading {
                        return;
                    }
                }
                if started.elapsed() >= LOAD_TIMEOUT {
                    page.fail_provisional(generation, "The page took too long to respond.");
                    return;
                }
                if cfg!(target_os = "macos") && started.elapsed() >= Duration::from_millis(600) {
                    if let (Some(target), Some(before)) = (&target, &native_before) {
                        if target != before && page.native_url().as_deref() == Some(before.as_str()) {
                            page.fail_provisional(generation, "The page could not be reached.");
                            return;
                        }
                    }
                }
            }
        });
    }

    fn fail_provisional(&self, generation: u64, message: &str) {
        {
            let mut state = lock(&self.state);
            if state.provisional != generation || !state.loading {
                return;
            }
            state.loading = false;
            state.progress = 0.0;
            state.intent = None;
            state.load_error = Some(message.to_string());
        }
        self.emit_state();
    }

    // MARK: Navigation

    /// `load(_:)`.
    fn load(self: &Arc<Self>, url: Url) -> Result<(), String> {
        if !navigation_allowed(&self.app, &url) {
            return Err(format!("{url} cannot be opened in the browser"));
        }
        let webview = self.webview()?;
        let native_before = self.native_url();
        lock(&self.state).intent = None;
        self.begin_provisional(Some(url.to_string()), native_before);
        webview.navigate(url).map_err(|error| {
            let message = format!("Could not open the page: {error}");
            let generation = lock(&self.state).provisional;
            self.fail_provisional(generation, &message);
            message
        })
    }

    /// `open(_:)`: what the user typed — an address (scheme added when
    /// missing), else a web search for the words.
    fn open(self: &Arc<Self>, text: &str) -> Result<(), String> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        let url = address_url(trimmed).unwrap_or_else(|| search_url(trimmed));
        self.load(url)
    }

    /// `goBack()` / `goForward()`: the page's own history, so same-document
    /// entries and the back-forward cache behave as in a browser.
    fn go(self: &Arc<Self>, intent: Intent) -> Result<(), String> {
        let webview = self.webview()?;
        {
            let state = lock(&self.state);
            let possible = match intent {
                Intent::Back => state.can_go_back(),
                Intent::Forward => state.can_go_forward(),
                Intent::Reload => true,
            };
            if !possible {
                return Ok(());
            }
        }
        let marker = self.marker();
        lock(&self.state).intent = Some(intent);
        let script = if intent == Intent::Back { "history.back()" } else { "history.forward()" };
        webview
            .eval(script)
            .map_err(|error| format!("Could not go {}: {error}", if intent == Intent::Back { "back" } else { "forward" }))?;
        // A same-document entry (pushState) never commits: notice it from the
        // address, and drop the intent when nothing happened at all.
        let weak = Arc::downgrade(self);
        tauri::async_runtime::spawn(async move {
            for delay in [250u64, 550, 700] {
                tokio::time::sleep(Duration::from_millis(delay)).await;
                let Some(page) = weak.upgrade() else { return };
                if lock(&page.state).intent != Some(intent) {
                    return;
                }
                page.refresh_location().await;
            }
            let Some(page) = weak.upgrade() else { return };
            let native = page.native_url();
            let mut state = lock(&page.state);
            let moving = state.loading || state.commits != marker.commits || state.starts != marker.starts || native != marker.native;
            if state.intent == Some(intent) && !moving {
                state.intent = None;
            }
        });
        Ok(())
    }

    /// `reload()`: nothing to do before the first page.
    fn reload(self: &Arc<Self>) -> Result<(), String> {
        if !self.has_page() {
            return Ok(());
        }
        let webview = self.webview()?;
        lock(&self.state).intent = Some(Intent::Reload);
        self.begin_provisional(None, None);
        webview.reload().map_err(|error| format!("Could not reload the page: {error}"))
    }

    /// `stop()`.
    fn stop(&self) -> Result<(), String> {
        let webview = self.webview()?;
        let _ = webview.eval("window.stop()");
        {
            let mut state = lock(&self.state);
            state.provisional += 1;
            state.loading = false;
            state.progress = 0.0;
            state.intent = None;
        }
        self.emit_state();
        Ok(())
    }

    /// Reads the document's address and title: same-document navigations
    /// (pushState, fragments) never reach the load hooks.
    async fn refresh_location(&self) {
        let commits = lock(&self.state).commits;
        let Ok(value) = self.eval_value(LOCATION_SCRIPT.to_string(), Duration::from_secs(3)).await else {
            return;
        };
        let Some(list) = value.as_array() else { return };
        let Some(href) = list.first().and_then(Value::as_str) else { return };
        let title = list.get(1).and_then(Value::as_str);
        let changed = {
            let mut state = lock(&self.state);
            // A commit since the question makes the answer stale.
            if state.commits != commits || state.provisional_pending() {
                return;
            }
            let mut changed = false;
            if let Some(title) = title {
                if state.title != title {
                    state.title = title.to_string();
                    changed = true;
                }
            }
            if let Ok(url) = Url::parse(href) {
                let address = page_address(&url);
                if address != state.url {
                    if let Some(address) = &address {
                        state.record(address);
                    }
                    state.url = address;
                    changed = true;
                }
            }
            changed
        };
        if changed {
            self.emit_state();
        }
    }

    fn spawn_location_poll(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(LOCATION_POLL).await;
                let Some(page) = weak.upgrade() else { return };
                if page.is_closed() {
                    return;
                }
                let (visible, loading) = {
                    let state = lock(&page.state);
                    (state.visible, state.loading)
                };
                if visible && !loading {
                    page.refresh_location().await;
                }
            }
        });
    }

    fn close(&self) {
        self.closed.store(true, Ordering::Relaxed);
        if let Some(webview) = self.webview.get() {
            if let Err(error) = webview.close() {
                debug(|| format!("browser: could not close the page: {error}"));
            }
        }
    }

    // MARK: Scripts

    /// One round trip: evaluates `source` and returns its (JSON) value.
    async fn eval_value(&self, source: String, timeout: Duration) -> Result<Value, String> {
        let webview = self.webview()?;
        let (sender, receiver) = tokio::sync::oneshot::channel::<String>();
        let sender = Mutex::new(Some(sender));
        webview
            .eval_with_callback(source, move |result| {
                if let Some(sender) = lock(&sender).take() {
                    let _ = sender.send(result);
                }
            })
            .map_err(|error| format!("could not run script in the page: {error}"))?;
        match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(raw)) => Ok(serde_json::from_str(&raw).unwrap_or(Value::Null)),
            Ok(Err(_)) => Err("the page did not answer".to_string()),
            Err(_) => Err("the page did not answer in time".to_string()),
        }
    }

    /// Runs a function body (`callAsyncJavaScript`) with named arguments and
    /// waits for its (possibly asynchronous) result.
    async fn run_script(&self, body: &str, arguments: &[(&str, Value)], timeout: Duration) -> Result<ScriptValue, ScriptError> {
        let serial = self.script_serial.fetch_add(1, Ordering::Relaxed);
        let id = format!("{:016x}-{serial}", self.script_salt);
        let id_literal = json_literal(&id);
        let names = arguments.iter().map(|(name, _)| *name).collect::<Vec<_>>().join(", ");
        let values = Value::Array(arguments.iter().map(|(_, value)| value.clone()).collect());
        let source = format!(
            "(function () {{\n{BOOTSTRAP_SCRIPT}\nreturn window.__ctxBrowser.start({id_literal}, async function ({names}) {{\n{body}\n}}, {values});\n}})()"
        );
        let started = self.eval_value(source, EVAL_TIMEOUT).await.map_err(ScriptError::Failed)?;
        if started.as_str() != Some("started") {
            return Err(ScriptError::Failed(self.parse_error(body).await));
        }
        let poll = format!(
            "(function () {{ var b = window.__ctxBrowser; return b ? b.poll({id_literal}) : {{ state: 'missing' }}; }})()"
        );
        let deadline = Instant::now() + timeout;
        let mut delay = 0u64;
        loop {
            if delay > 0 {
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
            delay = (delay * 2).clamp(10, 100);
            let answer = self.eval_value(poll.clone(), EVAL_TIMEOUT).await.map_err(ScriptError::Failed)?;
            match answer.get("state").and_then(Value::as_str) {
                Some("pending") => {}
                Some("done") => return Ok(ScriptValue::decode(answer.get("result"))),
                Some("error") => {
                    let message = answer.get("error").and_then(Value::as_str).unwrap_or("unknown error");
                    return Err(ScriptError::Failed(format!("script error: {message}")));
                }
                _ => return Err(ScriptError::Missing),
            }
            if Instant::now() >= deadline {
                let forget = format!("(function () {{ var b = window.__ctxBrowser; if (b) b.forget({id_literal}); return 0; }})()");
                let _ = self.eval_value(forget, EVAL_TIMEOUT).await;
                return Err(ScriptError::Failed(format!("the script did not finish within {} s", timeout.as_secs())));
            }
        }
    }

    /// The message for a body the page could not parse.
    async fn parse_error(&self, body: &str) -> String {
        let source = format!(
            "(function () {{ try {{ new (Object.getPrototypeOf(async function () {{}}).constructor)({}); return ''; }} catch (error) {{ return String(error && error.name ? error.name + ': ' + error.message : error); }} }})()",
            json_literal(body)
        );
        match self.eval_value(source, EVAL_TIMEOUT).await {
            Ok(Value::String(message)) if !message.is_empty() && !message.starts_with("EvalError") => format!("script error: {message}"),
            Ok(Value::String(message)) if message.is_empty() => "script error: the script did not start".to_string(),
            _ => "script error: the script could not be parsed".to_string(),
        }
    }

    /// `script(_:_:)`: an action script whose value is an object.
    async fn script_object(&self, body: &str, arguments: &[(&str, Value)]) -> Result<Map<String, Value>, ScriptError> {
        match self.run_script(body, arguments, SCRIPT_TIMEOUT).await? {
            ScriptValue::Json(Value::Object(object)) => Ok(object),
            _ => Ok(Map::new()),
        }
    }

    async fn reset_dialogs(&self) {
        let _ = self.eval_value(RESET_DIALOGS_SCRIPT.to_string(), EVAL_TIMEOUT).await;
    }

    async fn take_dialogs(&self) -> Vec<String> {
        match self.eval_value(TAKE_DIALOGS_SCRIPT.to_string(), EVAL_TIMEOUT).await {
            Ok(Value::Array(items)) => items.into_iter().filter_map(|item| item.as_str().map(str::to_string)).collect(),
            _ => Vec::new(),
        }
    }

    /// The document's own address (not the provisional one).
    async fn document_url(&self) -> Option<String> {
        match self.eval_value("String(location.href)".to_string(), Duration::from_secs(3)).await {
            Ok(Value::String(href)) => Some(href),
            _ => None,
        }
    }

    // MARK: Agent actions

    /// `perform(_:)`: one `browser/request` action; actions run one after another.
    async fn perform(self: &Arc<Self>, action: &Value) -> BrowserReply {
        let _turn = self.actions.lock().await;
        let kind = action.get("action").and_then(Value::as_str).unwrap_or("").to_string();
        self.reset_dialogs().await;
        let mut reply = match self.run(&kind, action).await {
            Ok(reply) => reply,
            Err(message) => return self.failure(message),
        };
        let dialogs = self.take_dialogs().await;
        if !dialogs.is_empty() {
            reply.content.push_str("\n\nPage dialogs (dismissed automatically): ");
            reply.content.push_str(&dialogs.join(" · "));
        }
        reply
    }

    async fn run(self: &Arc<Self>, kind: &str, action: &Value) -> Result<BrowserReply, String> {
        match kind {
            "navigate" => {
                let text = string_argument(action, "url").ok_or("navigate needs a valid `url`")?;
                let url = agent_url(&text).ok_or("navigate needs a valid `url`")?;
                let before = self.marker();
                self.load(url)?;
                self.await_load(LOAD_GRACE, &before).await;
                if let Some(error) = self.load_error() {
                    return Err(format!("could not load {text}: {error}"));
                }
                self.page_summary(NAVIGATE_TEXT_CHARS, true, None).await
            }
            "back" | "forward" | "reload" => {
                let before = self.marker();
                let grace = match kind {
                    "back" => {
                        if !lock(&self.state).can_go_back() {
                            return Err("there is no page to go back to".to_string());
                        }
                        self.go(Intent::Back)?;
                        HISTORY_GRACE
                    }
                    "forward" => {
                        if !lock(&self.state).can_go_forward() {
                            return Err("there is no page to go forward to".to_string());
                        }
                        self.go(Intent::Forward)?;
                        HISTORY_GRACE
                    }
                    _ => {
                        if !self.has_page() {
                            return Err(NO_PAGE.to_string());
                        }
                        self.reload()?;
                        LOAD_GRACE
                    }
                };
                self.await_load(grace, &before).await;
                if let Some(error) = self.load_error() {
                    return Err(error);
                }
                self.page_summary(NAVIGATE_TEXT_CHARS, true, None).await
            }
            "read" => {
                self.require_page()?;
                self.page_summary(MAX_TEXT_CHARS, false, None).await
            }
            "snapshot" => {
                self.require_page()?;
                let query = string_argument(action, "query").filter(|query| !query.is_empty());
                self.page_summary(0, true, query.as_deref()).await
            }
            "click" => {
                let reference = int_argument(action, "ref").ok_or("click needs `ref`")?;
                let before = self.marker();
                match self.script_object(CLICK_SCRIPT, &[("ref", json!(reference))]).await {
                    Ok(result) => {
                        if let Some(error) = result.get("error").and_then(Value::as_str) {
                            return Err(error.to_string());
                        }
                    }
                    // The click navigated before its result was collected.
                    Err(ScriptError::Missing) => {}
                    Err(ScriptError::Failed(message)) => return Err(message),
                }
                self.await_load(ACTION_GRACE, &before).await;
                self.page_summary(NAVIGATE_TEXT_CHARS, true, None).await
            }
            "type" => {
                let (Some(reference), Some(text)) = (int_argument(action, "ref"), string_argument(action, "text")) else {
                    return Err("type needs `ref` and `text`".to_string());
                };
                let submit = action.get("submit").and_then(Value::as_bool).unwrap_or(false);
                let before = self.marker();
                match self
                    .script_object(TYPE_SCRIPT, &[("ref", json!(reference)), ("text", json!(text)), ("submit", json!(submit))])
                    .await
                {
                    Ok(result) => {
                        if let Some(error) = result.get("error").and_then(Value::as_str) {
                            return Err(error.to_string());
                        }
                    }
                    Err(ScriptError::Missing) if submit => {}
                    Err(ScriptError::Missing) => return Err("the page changed while typing; take a new snapshot".to_string()),
                    Err(ScriptError::Failed(message)) => return Err(message),
                }
                if submit {
                    self.await_load(ACTION_GRACE, &before).await;
                    self.page_summary(NAVIGATE_TEXT_CHARS, true, None).await
                } else {
                    let count = text.chars().count();
                    Ok(self.success(format!("Typed {count} character{} into [{reference}].", if count == 1 { "" } else { "s" })))
                }
            }
            "select" => {
                let (Some(reference), Some(value)) = (int_argument(action, "ref"), string_argument(action, "value")) else {
                    return Err("select needs `ref` and `value`".to_string());
                };
                let result = self
                    .script_object(SELECT_SCRIPT, &[("ref", json!(reference)), ("value", json!(value))])
                    .await
                    .map_err(ScriptError::into_message)?;
                if let Some(error) = result.get("error").and_then(Value::as_str) {
                    return Err(error.to_string());
                }
                let label = result.get("label").and_then(Value::as_str).unwrap_or(&value).to_string();
                Ok(self.success(format!("Selected \"{label}\" in [{reference}].")))
            }
            "scroll" => {
                self.require_page()?;
                let direction = string_argument(action, "direction").unwrap_or_else(|| "down".to_string());
                let amount = int_argument(action, "amount").unwrap_or(0);
                let reference = int_argument(action, "ref").unwrap_or(-1);
                let result = self
                    .script_object(SCROLL_SCRIPT, &[("ref", json!(reference)), ("direction", json!(direction)), ("amount", json!(amount))])
                    .await
                    .map_err(ScriptError::into_message)?;
                if let Some(error) = result.get("error").and_then(Value::as_str) {
                    return Err(error.to_string());
                }
                tokio::time::sleep(Duration::from_millis(150)).await;
                let y = result.get("y").and_then(Value::as_f64).unwrap_or(0.0) as i64;
                let height = result.get("height").and_then(Value::as_f64).unwrap_or(0.0) as i64;
                Ok(self.success(format!("Scrolled to y={y} of {height}.")))
            }
            "evaluate" => {
                self.require_page()?;
                let source = string_argument(action, "script").ok_or("evaluate needs `script`")?;
                let value = self.run_script(&source, &[], EVALUATE_TIMEOUT).await.map_err(|error| match error {
                    ScriptError::Missing => "the page navigated before the script finished".to_string(),
                    ScriptError::Failed(message) => message,
                })?;
                Ok(self.success(value.render()))
            }
            "screenshot" => Err("screenshots are not supported in this app yet".to_string()),
            _ => Err(format!("unknown action `{kind}`")),
        }
    }

    fn require_page(&self) -> Result<(), String> {
        if self.has_page() {
            Ok(())
        } else {
            Err(NO_PAGE.to_string())
        }
    }

    fn load_error(&self) -> Option<String> {
        lock(&self.state).load_error.clone()
    }

    fn success(&self, content: String) -> BrowserReply {
        let (url, title) = self.current();
        BrowserReply { ok: true, content, url, title: Some(title), error: None }
    }

    fn failure(&self, message: String) -> BrowserReply {
        let (url, title) = self.current();
        BrowserReply { ok: false, content: String::new(), url, title: Some(title), error: Some(message) }
    }

    /// `awaitLoad(graceMs:)`: waits for a navigation to finish — one already
    /// under way, or one that starts within `grace` (a click that navigates).
    /// Tauri only reports commits and finishes, so a pending load also shows
    /// as a moved native address (WebKit) or a navigation request (Windows),
    /// and a same-document move as a changed `location.href`.
    async fn await_load(&self, grace: Duration, before: &Marker) {
        let started = Instant::now();
        let began = |state: &PageState| {
            state.loading || state.commits != before.commits || state.finishes != before.finishes || state.starts != before.starts
        };
        loop {
            let under_way = began(&lock(&self.state));
            if under_way {
                break;
            }
            if before.native.is_some() && self.native_url() != before.native {
                break;
            }
            if started.elapsed() >= grace {
                tokio::time::sleep(SETTLE).await;
                return;
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
        let mut last_document_check: Option<Instant> = None;
        while started.elapsed() < LOAD_TIMEOUT {
            let (loading, finished, requested) = {
                let state = lock(&self.state);
                (state.loading, state.finishes != before.finishes, state.starts != before.starts)
            };
            if loading {
                tokio::time::sleep(Duration::from_millis(60)).await;
                continue;
            }
            if finished {
                break;
            }
            if requested {
                // Windows: requested but not completed yet.
                tokio::time::sleep(Duration::from_millis(60)).await;
                continue;
            }
            let native = self.native_url();
            if native == before.native {
                // Nothing happened, or a provisional load fell back.
                break;
            }
            if last_document_check.is_none_or(|checked| checked.elapsed() >= Duration::from_millis(200)) {
                last_document_check = Some(Instant::now());
                if native.is_some() && self.document_url().await == native {
                    // A same-document navigation: no load follows.
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(60)).await;
        }
        tokio::time::sleep(SETTLE).await;
    }

    /// `pageSummary(textLimit:includeElements:query:)`.
    async fn page_summary(&self, text_limit: usize, include_elements: bool, query: Option<&str>) -> Result<BrowserReply, String> {
        let arguments = [
            ("query", json!(query.unwrap_or(""))),
            ("maxElements", json!(MAX_ELEMENTS)),
            ("maxText", json!(text_limit)),
        ];
        let result = match self.script_object(SNAPSHOT_SCRIPT, &arguments).await {
            Ok(result) => result,
            // The page moved on under the script: read the new one.
            Err(ScriptError::Missing) => {
                let before = self.marker();
                self.await_load(LOAD_GRACE, &before).await;
                self.script_object(SNAPSHOT_SCRIPT, &arguments).await.map_err(ScriptError::into_message)?
            }
            Err(ScriptError::Failed(message)) => return Err(message),
        };
        let (state_url, state_title) = self.current();
        let page_url = result.get("url").and_then(Value::as_str).map(str::to_string).or(state_url);
        let page_title = result.get("title").and_then(Value::as_str).map(str::to_string).unwrap_or(state_title);
        let mut parts: Vec<String> = Vec::new();
        if text_limit > 0 {
            let text = result.get("text").and_then(Value::as_str).unwrap_or("").trim().to_string();
            let length = text.encode_utf16().count() as u64;
            let total = result.get("textLength").and_then(Value::as_u64).unwrap_or(length);
            if text.is_empty() {
                parts.push("(the page has no visible text)".to_string());
            } else {
                parts.push(text);
                if total > length {
                    parts.push(format!("… [{} more characters; use read for the full text or scroll]", total - length));
                }
            }
        }
        if include_elements {
            let elements: Vec<String> = result
                .get("elements")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            let count = elements.len() as u64;
            let total = result.get("total").and_then(Value::as_u64).unwrap_or(count);
            if elements.is_empty() {
                parts.push(match query {
                    None => "Interactive elements: none found.".to_string(),
                    Some(query) => format!("Interactive elements matching \"{query}\": none."),
                });
            } else {
                let header = match query {
                    None => format!(
                        "Interactive elements ({count}{}): click, type and select take these numbers.",
                        if total > count { format!(" of {total}") } else { String::new() }
                    ),
                    Some(query) => format!("Interactive elements matching \"{query}\" ({count} of {total}):"),
                };
                parts.push(format!("{header}\n{}", elements.join("\n")));
            }
        }
        Ok(BrowserReply { ok: true, content: parts.join("\n\n"), url: page_url, title: Some(page_title), error: None })
    }
}

impl PageState {
    /// A load this module started that has not committed yet: the document
    /// still shows the old page, so its address says nothing new.
    fn provisional_pending(&self) -> bool {
        self.loading && !self.committed_since_finish
    }
}

// MARK: - Script values

/// A script's result (`callAsyncJavaScript`'s value).
#[derive(Debug, Clone, PartialEq)]
enum ScriptValue {
    Undefined,
    String(String),
    Json(Value),
    /// A value JSON cannot hold, as `String(value)`.
    Text(String),
}

impl ScriptValue {
    fn decode(raw: Option<&Value>) -> ScriptValue {
        let Some(raw) = raw else { return ScriptValue::Undefined };
        let value = raw.get("value").and_then(Value::as_str).unwrap_or("").to_string();
        match raw.get("kind").and_then(Value::as_str) {
            Some("string") => ScriptValue::String(value),
            Some("json") => serde_json::from_str(&value).map(ScriptValue::Json).unwrap_or(ScriptValue::Text(value)),
            Some("text") => ScriptValue::Text(value),
            _ => ScriptValue::Undefined,
        }
    }

    /// `render(_:)`: strings as they are, objects and arrays pretty-printed
    /// with sorted keys, other values in their JSON spelling.
    fn render(&self) -> String {
        match self {
            ScriptValue::Undefined => "undefined".to_string(),
            ScriptValue::String(text) | ScriptValue::Text(text) => text.clone(),
            ScriptValue::Json(value @ (Value::Object(_) | Value::Array(_))) => {
                serde_json::to_string_pretty(&sorted(value)).unwrap_or_else(|_| value.to_string())
            }
            ScriptValue::Json(value) => value.to_string(),
        }
    }
}

fn sorted(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut keys: Vec<&String> = object.keys().collect();
            keys.sort();
            let mut out = Map::new();
            for key in keys {
                out.insert(key.clone(), sorted(&object[key]));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ScriptError {
    /// The document the script ran in is gone (the page navigated).
    Missing,
    Failed(String),
}

impl ScriptError {
    fn into_message(self) -> String {
        match self {
            ScriptError::Missing => "the page changed before the script finished; take a new snapshot".to_string(),
            ScriptError::Failed(message) => message,
        }
    }
}

// MARK: - Addresses

/// A page's address for the state; null for the blank start page.
fn page_address(url: &Url) -> Option<String> {
    if url.scheme() == "about" {
        None
    } else {
        Some(url.to_string())
    }
}

fn is_web_url(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

/// `BrowserWorkspace.url(from:)`: `example.com/docs` → https://example.com/docs;
/// words with spaces or without a dot are not addresses.
fn address_url(text: &str) -> Option<Url> {
    if text.contains("://") {
        let url = Url::parse(text).ok()?;
        return (is_web_url(&url) && url.host_str().is_some()).then_some(url);
    }
    if text.contains(' ') {
        return None;
    }
    let host = text.split('/').next().unwrap_or(text);
    let bare = host.split(':').next().unwrap_or(host);
    if !(bare.contains('.') || bare == "localhost" || bare.ends_with(".localhost")) {
        return None;
    }
    Url::parse(&format!("https://{text}")).ok()
}

/// A DuckDuckGo search for `text` (`open(_:)`'s fallback).
fn search_url(text: &str) -> Url {
    let mut url = Url::parse("https://duckduckgo.com/").expect("the search address is a URL");
    url.query_pairs_mut().append_pair("q", text);
    url
}

/// The agent's `navigate` address: http(s) only (the tool already adds
/// `https://` to a bare host; this does the same for safety).
fn agent_url(text: &str) -> Option<Url> {
    let trimmed = text.trim();
    if trimmed.contains("://") {
        let url = Url::parse(trimmed).ok()?;
        return (is_web_url(&url) && url.host_str().is_some()).then_some(url);
    }
    address_url(trimmed)
}

/// Navigations the page may make: web pages and the inert schemes frames
/// use, never the app's own origins (its pages, IPC and the asset protocol,
/// which serves local files) or other local schemes.
fn navigation_allowed(app: &AppHandle, url: &Url) -> bool {
    match url.scheme() {
        "http" | "https" => !is_app_origin(app, url),
        "about" | "data" | "blob" | "javascript" => true,
        _ => false,
    }
}

fn is_app_origin(app: &AppHandle, url: &Url) -> bool {
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    if matches!(host.as_str(), "tauri.localhost" | "ipc.localhost" | "asset.localhost") {
        return true;
    }
    if cfg!(debug_assertions) {
        if let Some(dev) = app.config().build.dev_url.as_ref() {
            return dev.scheme() == url.scheme() && dev.host_str() == url.host_str() && dev.port_or_known_default() == url.port_or_known_default();
        }
    }
    false
}

// MARK: - Arguments

fn json_literal(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string())
}

/// `JSONValue.stringValue`.
fn string_argument(action: &Value, key: &str) -> Option<String> {
    action.get(key).and_then(Value::as_str).map(str::to_string)
}

/// `JSONValue.intValue` (numbers truncate; numeric strings are accepted too).
fn int_argument(action: &Value, key: &str) -> Option<i64> {
    match action.get(key)? {
        Value::Number(number) => number.as_i64().or_else(|| number.as_f64().filter(|value| value.is_finite()).map(|value| value.trunc() as i64)),
        Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    }
}

// MARK: - Commands

/// `browser_create({ project_id, bounds, visible? })`: the project's page,
/// created on first use, placed at `bounds` (shown unless `visible` is false).
#[tauri::command(rename_all = "snake_case")]
pub async fn browser_create(
    app: AppHandle,
    browsers: State<'_, Browsers>,
    project_id: String,
    bounds: Bounds,
    visible: Option<bool>,
) -> Result<BrowserState, String> {
    let visible = visible.unwrap_or(true);
    let page = browsers.ensure(&app, &project_id, Some(bounds), visible).await?;
    page.set_layout(Some(bounds), Some(visible))?;
    Ok(page.snapshot())
}

/// `browser_state({ project_id })`: the page's state, or null when it has none.
#[tauri::command(rename_all = "snake_case")]
pub async fn browser_state(browsers: State<'_, Browsers>, project_id: String) -> Result<Option<BrowserState>, String> {
    Ok(browsers.get(&project_id).map(|page| page.snapshot()))
}

/// `browser_navigate({ project_id, url })`: what the user typed in the
/// address field — an address, or a web search for the words.
#[tauri::command(rename_all = "snake_case")]
pub async fn browser_navigate(app: AppHandle, browsers: State<'_, Browsers>, project_id: String, url: String) -> Result<(), String> {
    let page = browsers.ensure(&app, &project_id, None, false).await?;
    page.open(&url)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn browser_back(browsers: State<'_, Browsers>, project_id: String) -> Result<(), String> {
    browsers.require(&project_id)?.go(Intent::Back)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn browser_forward(browsers: State<'_, Browsers>, project_id: String) -> Result<(), String> {
    browsers.require(&project_id)?.go(Intent::Forward)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn browser_reload(browsers: State<'_, Browsers>, project_id: String) -> Result<(), String> {
    browsers.require(&project_id)?.reload()
}

#[tauri::command(rename_all = "snake_case")]
pub async fn browser_stop(browsers: State<'_, Browsers>, project_id: String) -> Result<(), String> {
    browsers.require(&project_id)?.stop()
}

/// `browser_set_bounds({ project_id, bounds })`: follows the tab's area.
#[tauri::command(rename_all = "snake_case")]
pub async fn browser_set_bounds(browsers: State<'_, Browsers>, project_id: String, bounds: Bounds) -> Result<(), String> {
    match browsers.get(&project_id) {
        Some(page) => page.set_layout(Some(bounds), None),
        None => Ok(()),
    }
}

/// `browser_show({ project_id })`: puts the page back over the tab's area.
#[tauri::command(rename_all = "snake_case")]
pub async fn browser_show(browsers: State<'_, Browsers>, project_id: String) -> Result<(), String> {
    match browsers.get(&project_id) {
        Some(page) => page.set_layout(None, Some(true)),
        None => Ok(()),
    }
}

/// `browser_hide({ project_id })`: parks the page out of the window (another
/// Inspector tab is selected); it keeps running.
#[tauri::command(rename_all = "snake_case")]
pub async fn browser_hide(browsers: State<'_, Browsers>, project_id: String) -> Result<(), String> {
    match browsers.get(&project_id) {
        Some(page) => page.set_layout(None, Some(false)),
        None => Ok(()),
    }
}

/// `browser_destroy({ project_id })`: `tearDown()` when the project closes.
#[tauri::command(rename_all = "snake_case")]
pub async fn browser_destroy(browsers: State<'_, Browsers>, project_id: String) -> Result<(), String> {
    if let Some(page) = browsers.remove(&project_id) {
        page.close();
    }
    Ok(())
}

/// `browser_perform({ project_id, action }) -> { ok, content, url, title, error }`:
/// one action of the agent's `browser` tool, run in the project's page
/// (created out of sight when the tab has never shown it).
#[tauri::command(rename_all = "snake_case")]
pub async fn browser_perform(app: AppHandle, browsers: State<'_, Browsers>, project_id: String, action: Value) -> Result<BrowserReply, String> {
    let page = browsers.ensure(&app, &project_id, None, false).await?;
    Ok(page.perform(&action).await)
}

// MARK: - Scripts

/// Installed in every page before its own scripts (and again by each action
/// when the page has none): the action runner and the dialog handlers.
const BOOTSTRAP_SCRIPT: &str = r##"(function () {
  if (window.__ctxBrowser) return;
  var stringify = JSON.stringify;
  var pending = Object.create(null);
  var dialogs = [];
  function text(value) {
    try { return value === undefined ? '' : String(value); } catch (_) { return ''; }
  }
  function describe(error) {
    try {
      if (error && typeof error === 'object' && 'message' in error) {
        return (error.name ? String(error.name) : 'Error') + ': ' + String(error.message);
      }
      return String(error);
    } catch (_) {
      return 'unknown error';
    }
  }
  function encode(value) {
    if (value === undefined) return { kind: 'undefined' };
    if (typeof value === 'string') return { kind: 'string', value: value };
    try {
      var json = stringify(value);
      if (typeof json === 'string') return { kind: 'json', value: json };
    } catch (_) {}
    try { return { kind: 'text', value: String(value) }; } catch (_) { return { kind: 'text', value: Object.prototype.toString.call(value) }; }
  }
  var api = {
    start: function (id, run, args) {
      var slot = { done: false };
      pending[id] = slot;
      try {
        Promise.resolve(run.apply(null, args)).then(
          function (value) { slot.result = encode(value); slot.done = true; },
          function (error) { slot.error = describe(error); slot.done = true; }
        );
      } catch (error) {
        slot.error = describe(error);
        slot.done = true;
      }
      return 'started';
    },
    poll: function (id) {
      var slot = pending[id];
      if (!slot) return { state: 'missing' };
      if (!slot.done) return { state: 'pending' };
      delete pending[id];
      if (slot.error !== undefined) return { state: 'error', error: slot.error };
      return { state: 'done', result: slot.result };
    },
    forget: function (id) { delete pending[id]; },
    resetDialogs: function () { dialogs.length = 0; return 0; },
    takeDialogs: function () { var list = dialogs.slice(); dialogs.length = 0; return list; }
  };
  try {
    Object.defineProperty(window, '__ctxBrowser', { value: api, configurable: false, enumerable: false, writable: false });
  } catch (_) {
    window.__ctxBrowser = api;
  }
  window.alert = function (message) { dialogs.push('alert: ' + text(message)); };
  window.confirm = function (message) { dialogs.push('confirm (answered Cancel): ' + text(message)); return false; };
  window.prompt = function (message) { dialogs.push('prompt (dismissed): ' + text(message)); return null; };
})();"##;

const LOCATION_SCRIPT: &str = "[String(location.href), String(document.title)]";

const RESET_DIALOGS_SCRIPT: &str = "(function () { var b = window.__ctxBrowser; return b ? b.resetDialogs() : 0; })()";

const TAKE_DIALOGS_SCRIPT: &str = "(function () { var b = window.__ctxBrowser; return b ? b.takeDialogs() : []; })()";

/// Numbers the visible interactive elements with `data-ctx-ref` and returns
/// the page text and one line per element (`snapshotScript`).
const SNAPSHOT_SCRIPT: &str = r##"
const selector = 'a[href], button, input, select, textarea, summary, [role="button"], [role="link"], [role="tab"], [role="menuitem"], [role="menuitemcheckbox"], [role="menuitemradio"], [role="checkbox"], [role="radio"], [role="switch"], [role="textbox"], [role="searchbox"], [role="combobox"], [role="option"], [role="slider"], [contenteditable=""], [contenteditable="true"], [onclick], [tabindex]:not([tabindex="-1"])';
const vh = window.innerHeight;
function visible(el) {
    const r = el.getBoundingClientRect();
    if (r.width === 0 && r.height === 0) return false;
    const cs = getComputedStyle(el);
    return cs.visibility !== 'hidden' && cs.display !== 'none' && cs.opacity !== '0';
}
function role(el) {
    const r = el.getAttribute('role');
    if (r) return r;
    const tag = el.tagName.toLowerCase();
    if (tag === 'a') return 'link';
    if (tag === 'input') {
        const t = (el.type || 'text').toLowerCase();
        if (t === 'submit' || t === 'button' || t === 'reset' || t === 'image') return 'button';
        if (t === 'checkbox' || t === 'radio' || t === 'range' || t === 'file' || t === 'color') return t;
        return 'textbox';
    }
    if (tag === 'textarea') return 'textbox';
    if (el.isContentEditable) return 'textbox';
    return tag;
}
function name(el) {
    const aria = el.getAttribute('aria-label');
    if (aria) return aria;
    const labelled = el.getAttribute('aria-labelledby');
    if (labelled) {
        const text = labelled.split(/\s+/).map(id => { const n = document.getElementById(id); return n ? n.innerText : ''; }).join(' ').trim();
        if (text) return text;
    }
    if (el.labels && el.labels.length && el.labels[0].innerText.trim()) return el.labels[0].innerText.trim();
    const own = (el.innerText || '').trim();
    if (own) return own;
    return (el.getAttribute('placeholder') || el.getAttribute('title') || el.getAttribute('alt') || el.getAttribute('name') || el.value || '').toString().trim();
}
document.querySelectorAll('[data-ctx-ref]').forEach(e => e.removeAttribute('data-ctx-ref'));
const q = (query || '').toLowerCase();
const lines = [];
let n = 0;
for (const el of document.querySelectorAll(selector)) {
    if (!visible(el)) continue;
    const ref = n++;
    el.setAttribute('data-ctx-ref', String(ref));
    if (lines.length >= maxElements) continue;
    const tag = el.tagName.toLowerCase();
    const r = el.getBoundingClientRect();
    let extra = '';
    if (tag === 'a' && el.href) extra = ' → ' + String(el.href).slice(0, 120);
    if ((tag === 'input' || tag === 'textarea') && el.value && el.type !== 'password') extra = ' value="' + String(el.value).slice(0, 60) + '"';
    if (tag === 'input' && el.type === 'password') extra = ' (password field)';
    if (tag === 'input' && (el.type === 'checkbox' || el.type === 'radio')) extra = el.checked ? ' (checked)' : ' (unchecked)';
    if (tag === 'select') extra = ' options: ' + Array.from(el.options).map(o => o.text.trim()).slice(0, 12).join(' | ') + (el.options.length > 12 ? ' | …' : '');
    if (el.disabled) extra += ' (disabled)';
    const line = role(el) + ' "' + name(el).replace(/\s+/g, ' ').slice(0, 80) + '"' + extra;
    if (q && !line.toLowerCase().includes(q)) continue;
    lines.push('[' + ref + '] ' + line + (r.bottom < 0 || r.top > vh ? ' (offscreen)' : ''));
}
const text = (document.body ? document.body.innerText : '').replace(/[ \t]+\n/g, '\n').replace(/\n{3,}/g, '\n\n').trim();
return { url: location.href, title: document.title, text: maxText > 0 ? text.slice(0, maxText) : '', textLength: text.length, elements: lines, total: n };
"##;

/// `clickScript`.
const CLICK_SCRIPT: &str = r##"
const el = document.querySelector('[data-ctx-ref="' + ref + '"]');
if (!el) return { error: 'no element [' + ref + '] on this page; take a new snapshot' };
if (el.disabled) return { error: 'element [' + ref + '] is disabled' };
el.scrollIntoView({ block: 'center', inline: 'nearest' });
const r = el.getBoundingClientRect();
const init = { bubbles: true, cancelable: true, view: window, clientX: r.left + r.width / 2, clientY: r.top + r.height / 2 };
el.dispatchEvent(new MouseEvent('mousedown', init));
if (typeof el.focus === 'function') el.focus();
el.dispatchEvent(new MouseEvent('mouseup', init));
el.click();
return { ok: true };
"##;

/// `typeScript`.
const TYPE_SCRIPT: &str = r##"
const el = document.querySelector('[data-ctx-ref="' + ref + '"]');
if (!el) return { error: 'no element [' + ref + '] on this page; take a new snapshot' };
if (el.disabled || el.readOnly) return { error: 'element [' + ref + '] cannot be edited' };
el.scrollIntoView({ block: 'center', inline: 'nearest' });
if (typeof el.focus === 'function') el.focus();
const tag = el.tagName.toLowerCase();
if (el.isContentEditable) {
    el.textContent = text;
    el.dispatchEvent(new InputEvent('input', { bubbles: true, data: text, inputType: 'insertText' }));
} else if (tag === 'input' || tag === 'textarea') {
    const proto = tag === 'textarea' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    const setter = Object.getOwnPropertyDescriptor(proto, 'value');
    if (setter && setter.set) setter.set.call(el, text); else el.value = text;
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
} else {
    return { error: 'element [' + ref + '] is not a text field' };
}
if (submit) {
    const key = { key: 'Enter', code: 'Enter', keyCode: 13, which: 13, bubbles: true, cancelable: true };
    const proceed = el.dispatchEvent(new KeyboardEvent('keydown', key));
    el.dispatchEvent(new KeyboardEvent('keypress', key));
    el.dispatchEvent(new KeyboardEvent('keyup', key));
    if (proceed && el.form) {
        if (typeof el.form.requestSubmit === 'function') el.form.requestSubmit(); else el.form.submit();
    }
}
return { ok: true };
"##;

/// `selectScript`.
const SELECT_SCRIPT: &str = r##"
const el = document.querySelector('[data-ctx-ref="' + ref + '"]');
if (!el) return { error: 'no element [' + ref + '] on this page; take a new snapshot' };
if (el.tagName.toLowerCase() !== 'select') return { error: 'element [' + ref + '] is not a select' };
const wanted = String(value).trim().toLowerCase();
const option = Array.from(el.options).find(o => o.value.toLowerCase() === wanted) || Array.from(el.options).find(o => o.text.trim().toLowerCase() === wanted);
if (!option) return { error: 'no option "' + value + '" in [' + ref + ']; options: ' + Array.from(el.options).map(o => o.text.trim()).join(' | ') };
el.value = option.value;
el.dispatchEvent(new Event('input', { bubbles: true }));
el.dispatchEvent(new Event('change', { bubbles: true }));
return { ok: true, label: option.text.trim() };
"##;

/// `scrollScript`.
const SCROLL_SCRIPT: &str = r##"
if (ref >= 0) {
    const el = document.querySelector('[data-ctx-ref="' + ref + '"]');
    if (!el) return { error: 'no element [' + ref + '] on this page; take a new snapshot' };
    el.scrollIntoView({ block: 'center', inline: 'nearest', behavior: 'instant' });
} else {
    const step = amount > 0 ? amount : Math.max(200, window.innerHeight - 80);
    if (direction === 'top') window.scrollTo(0, 0);
    else if (direction === 'bottom') window.scrollTo(0, document.documentElement.scrollHeight);
    else window.scrollBy(0, direction === 'up' ? -step : step);
}
return { ok: true, y: Math.round(window.scrollY), height: Math.round(document.documentElement.scrollHeight) };
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_follow_the_swift_rules() {
        assert_eq!(address_url("example.com/docs").map(|url| url.to_string()), Some("https://example.com/docs".to_string()));
        assert_eq!(address_url("localhost:3000").map(|url| url.to_string()), Some("https://localhost:3000/".to_string()));
        assert_eq!(address_url("app.localhost").map(|url| url.to_string()), Some("https://app.localhost/".to_string()));
        assert_eq!(address_url("http://example.com").map(|url| url.to_string()), Some("http://example.com/".to_string()));
        assert!(address_url("rust borrow checker").is_none());
        assert!(address_url("hello").is_none());
        assert!(address_url("ftp://example.com").is_none());
        assert!(address_url("file:///etc/passwd").is_none());
    }

    #[test]
    fn searches_encode_the_words() {
        assert_eq!(search_url("c++ tutorial").to_string(), "https://duckduckgo.com/?q=c%2B%2B+tutorial");
    }

    #[test]
    fn agent_addresses_are_web_only() {
        assert!(agent_url("https://example.com").is_some());
        assert!(agent_url("example.com").is_some());
        assert!(agent_url("javascript://alert(1)").is_none());
        assert!(agent_url("file:///etc/passwd").is_none());
    }

    #[test]
    fn history_moves_like_a_back_list() {
        let mut state = PageState {
            url: None,
            title: String::new(),
            loading: false,
            progress: 0.0,
            load_error: None,
            history: Vec::new(),
            index: 0,
            intent: None,
            commits: 0,
            finishes: 0,
            starts: 0,
            committed_since_finish: false,
            provisional: 0,
            bounds: Bounds::DETACHED,
            visible: false,
        };
        state.record("https://a/");
        assert!(!state.can_go_back());
        state.record("https://b/");
        state.record("https://b/");
        assert_eq!(state.history.len(), 2);
        state.intent = Some(Intent::Back);
        state.record("https://a/");
        assert!(state.can_go_forward() && !state.can_go_back());
        state.record("https://c/");
        assert_eq!(state.history, vec!["https://a/".to_string(), "https://c/".to_string()]);
        assert!(!state.can_go_forward());
    }

    #[test]
    fn script_values_render_like_swift() {
        assert_eq!(ScriptValue::Undefined.render(), "undefined");
        assert_eq!(ScriptValue::String("hi".into()).render(), "hi");
        assert_eq!(ScriptValue::Json(json!(3)).render(), "3");
        assert_eq!(ScriptValue::Json(json!({ "b": 1, "a": [true] })).render(), "{\n  \"a\": [\n    true\n  ],\n  \"b\": 1\n}");
        let decoded = ScriptValue::decode(Some(&json!({ "kind": "json", "value": "{\"x\":1}" })));
        assert_eq!(decoded, ScriptValue::Json(json!({ "x": 1 })));
    }

    #[test]
    fn integer_arguments_truncate() {
        let action = json!({ "ref": 3.7, "amount": "120", "bad": true });
        assert_eq!(int_argument(&action, "ref"), Some(3));
        assert_eq!(int_argument(&action, "amount"), Some(120));
        assert_eq!(int_argument(&action, "bad"), None);
        assert_eq!(int_argument(&action, "missing"), None);
    }

    #[test]
    fn labels_are_safe() {
        assert_eq!(label_safe("proj 1/é"), "proj_1/_");
    }
}
