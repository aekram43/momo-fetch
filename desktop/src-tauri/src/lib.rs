//! MOMO WORK desktop shell.
//!
//! Wraps the exported web UI and supervises a `momo-fetch --gateway` child.
//! The web UI is byte-identical to the browser build — everything
//! desktop-specific lives here.

mod deeplink;
mod gateway;
mod menu;
mod secrets;
mod shell_path;
mod workspaces;

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use tauri::{Manager, State};

use gateway::Supervisor;

/// Everything the shell needs to answer the frontend and shut down cleanly.
pub struct AppState {
    pub supervisor: Mutex<Option<Supervisor>>,
    /// The project the gateway is rooted at right now.
    ///
    /// Not derivable from `default_project_dir`: once someone opens another
    /// workspace, that function still answers with the app-data one, and a
    /// restart that trusted it would silently move them back.
    pub project: Mutex<Option<PathBuf>>,
    /// Resolved gateway URL, or `None` if startup failed.
    pub url: Mutex<Option<String>>,
    /// Populated when startup failed, so the UI can show something useful.
    pub startup_error: Mutex<Option<String>>,
}

/// Locate the `momo-fetch` binary.
///
/// Bundled as a resource for release (T10, per target triple), but a developer
/// running `cargo tauri dev` has it in the workspace `target/` instead. Try the
/// bundle, then the dev paths, then `PATH`.
fn resolve_binary(app: &tauri::AppHandle) -> Option<PathBuf> {
    if let Ok(dir) = app.path().resource_dir() {
        // `resources: ["binaries/"]` in tauri.conf.json copies the *directory*,
        // so the binary lands at `<resources>/binaries/momo-fetch`, not directly
        // in the resource root. Check both — a future config change that
        // flattens it should not silently break startup.
        for sub in ["binaries", "."] {
            for name in ["momo-fetch", "momo-fetch.exe"] {
                let candidate = dir.join(sub).join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from);
    if let Some(root) = root {
        for rel in ["target/release/momo-fetch", "target/debug/momo-fetch"] {
            let candidate = root.join(rel);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    which_on_path("momo-fetch")
}

/// Where the recent-workspace list is kept.
///
/// Beside the app's own config rather than inside any workspace: it is a fact
/// about this machine, and putting it in a project would make it travel with a
/// repository someone shares.
fn recents_store(app: &tauri::AppHandle) -> PathBuf {
    app.path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("recent-workspaces.json")
}

/// **T4c** — workspaces this machine has opened, most recent first.
///
/// Filtered to the ones that are still workspaces, so a folder that was deleted
/// or emptied since does not offer a click that can only fail. The one that is
/// open right now is left out — it is already on screen.
#[tauri::command]
fn recent_workspaces(app: tauri::AppHandle, state: State<'_, AppState>) -> Vec<String> {
    let current = state.project.lock().ok().and_then(|p| p.clone());

    workspaces::list(&recents_store(&app))
        .into_iter()
        .filter(|p| is_workspace(p))
        .filter(|p| Some(p) != current.as_ref())
        .map(|p| p.display().to_string())
        .collect()
}

/// What a workspace gets when there is no running gateway to inherit from.
///
/// A free model on purpose: the very first launch has no key for anything, and
/// a default the user can actually reach with a free account beats one that
/// only names a paid provider.
const FALLBACK_PROVIDER: &str = "openrouter";
const FALLBACK_MODEL: &str = "nvidia/nemotron-3-ultra-550b-a55b:free";

/// The provider and model the running gateway is using, for a workspace to
/// inherit.
///
/// Asked of the gateway rather than read from a settings file: the user may
/// have switched model mid-session, and `/health` is the only thing that knows
/// what is actually loaded. Falls back rather than failing — a workspace that
/// gets the default provider still opens, and one that is not created at all
/// because a health probe timed out is a worse trade.
///
/// The request itself runs on `spawn_blocking`: `ureq` is a blocking client
/// (see the `Cargo.toml` note by its dependency), and this function is called
/// from an async command — running it inline would tie up a runtime worker
/// thread for the length of the HTTP round trip.
async fn inherited_selection(state: &State<'_, AppState>) -> (String, String) {
    let fallback = || (FALLBACK_PROVIDER.to_string(), FALLBACK_MODEL.to_string());

    let Some(url) = state.url.lock().ok().and_then(|u| u.clone()) else {
        return fallback();
    };

    let found = tauri::async_runtime::spawn_blocking(move || {
        let resp = ureq::get(&format!("{url}/health"))
            .timeout(Duration::from_secs(3))
            .call()
            .ok()?;

        // `into_string` + serde rather than ureq's `into_json`, which is behind
        // a feature this crate does not enable for one small response.
        let body = resp.into_string().ok()?;
        let health: serde_json::Value = serde_json::from_str(&body).ok()?;

        match (
            health.get("provider").and_then(|v| v.as_str()),
            health.get("model").and_then(|v| v.as_str()),
        ) {
            (Some(provider), Some(model)) if !provider.is_empty() && !model.is_empty() => {
                Some((provider.to_string(), model.to_string()))
            }
            _ => None,
        }
    })
    .await
    .ok()
    .flatten();

    found.unwrap_or_else(fallback)
}

/// Where the agent is rooted when the user has not chosen a project.
///
/// **Deliberately not `$HOME`.** The project root *is* the sandbox root — it is
/// what `/v2/files` will serve and what the agent's file tools can reach — so
/// defaulting to the home directory would hand a fresh install read access to
/// everything the user owns. A dedicated, created-on-first-run workspace is the
/// conservative default; "open another project" (T4) is the one-click escape.
fn default_project_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("no app data directory: {e}"))?;
    let workspace = base.join("workspace");
    std::fs::create_dir_all(&workspace)
        .map_err(|e| format!("could not create {}: {e}", workspace.display()))?;
    seed_workspace(&workspace, FALLBACK_PROVIDER, FALLBACK_MODEL);
    Ok(workspace)
}

/// What a workspace is, in one place.
///
/// `.harness/settings.json` and nothing else: it is the file whose absence
/// makes the harness fall back to `anthropic` and exit with "Secret not found",
/// so it is exactly the difference between a directory that boots and one that
/// does not.
pub fn is_workspace(dir: &std::path::Path) -> bool {
    dir.join(".harness").join("settings.json").is_file()
}

/// Give a fresh workspace enough config to start.
///
/// Without a `settings.json` the harness falls back to `anthropic` and dies with
/// "Secret not found for 'anthropic'" — a confusing first run when the user may
/// have configured a different provider entirely. Seeding a default at least
/// makes the failure name the provider it will actually use.
///
/// `provider`/`model` are inherited from the running gateway when there is one,
/// so a workspace created from a working session works too. A new workspace that
/// opens onto "Secret not found" for a provider the user has never used is a
/// worse first impression than the one this exists to prevent.
///
/// **Only non-secret config is written.** The API key is never seeded, guessed
/// or copied from elsewhere on disk; the user supplies it, and the boot screen
/// says how.
fn seed_workspace(dir: &std::path::Path, provider: &str, model: &str) {
    let harness = dir.join(".harness");
    if harness.join("settings.json").exists() {
        return;
    }
    if std::fs::create_dir_all(&harness).is_err() {
        return;
    }
    let settings = serde_json::json!({
        "permission_mode": "strict",
        "default_provider": provider,
        "default_model": model,
    });
    let rendered = serde_json::to_string_pretty(&settings).unwrap_or_default();
    let _ = std::fs::write(harness.join("settings.json"), format!("{rendered}\n"));

    // A pointer, not a secret. `dotenvy` reads `.env` from the working
    // directory, which is this workspace.
    let env_example = dir.join(".env.example");
    if !env_example.exists() {
        let _ = std::fs::write(
            &env_example,
            "# Rename to .env and fill in one of these.\n             OPENROUTER_API_KEY=\n             ANTHROPIC_API_KEY=\n             ZAI_API_KEY=\n",
        );
    }
}

/// `gateway::await_ready`, off the async runtime.
///
/// It busy-polls with blocking `ureq` calls and `std::thread::sleep` for up to
/// `timeout` — fine on the dedicated OS thread `setup` starts it from, but
/// called directly from an async command it would pin a runtime worker thread
/// for the whole poll. `spawn_blocking` runs it on the blocking pool instead.
async fn wait_until_ready(url: &str, timeout: Duration) -> bool {
    let url = url.to_string();
    tauri::async_runtime::spawn_blocking(move || gateway::await_ready(&url, timeout))
        .await
        .unwrap_or(false)
}

fn which_on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|p| p.is_file())
    })
}

// ── Commands exposed to the web UI ─────────────────────────────

/// **T3** — the gateway URL, read at runtime.
///
/// The frontend cannot learn this at build time: `output: 'export'` inlines env
/// vars when the bundle is compiled, but the port is assigned by the kernel at
/// launch. The shell injects `window.__GATEWAY_URL__` before the bundle runs;
/// this command is the fallback for anything that misses the injection.
#[tauri::command]
fn gateway_url(state: State<'_, AppState>) -> Option<String> {
    let url = state.url.lock().ok().and_then(|u| u.clone());
    log::info!("gateway_url command → {url:?}");
    url
}

#[tauri::command]
fn startup_error(state: State<'_, AppState>) -> Option<String> {
    state.startup_error.lock().ok().and_then(|e| e.clone())
}

/// Last lines of gateway stderr, for the crash screen.
#[tauri::command]
fn gateway_stderr(state: State<'_, AppState>) -> String {
    state
        .supervisor
        .lock()
        .ok()
        .and_then(|s| s.as_ref().map(|s| s.stderr_tail()))
        .unwrap_or_default()
}

/// **T4** — re-root the gateway at a new project directory.
///
/// Consequential: the sandbox, memory vault and session DB all follow the
/// project root. The confirm step lives in the UI; by the time this is called
/// the user has agreed.
#[tauri::command]
async fn open_project(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<String, String> {
    let dir = PathBuf::from(&path);
    if !dir.is_dir() {
        return Err(format!("{path} is not a directory."));
    }

    // **Checked before the old gateway is touched.** Opening a directory that
    // is not a workspace used to shut the running gateway down and then fail to
    // start one — the harness creates `memory-vault/` and exits with "Secret
    // not found for 'anthropic'", because with no settings.json it falls back
    // to a provider the user may never have configured. The app was left with
    // no gateway at all and a toast that said "Could not open that directory".
    // Refusing here costs nothing and keeps the session alive.
    if !is_workspace(&dir) {
        return Err(format!(
            "{path} is not a workspace — it has no .harness/settings.json. \
             Use \"create workspace\" to make one."
        ));
    }

    // Stop the old gateway *before* starting a new one: two gateways over one
    // `.harness/` fight over the session DB — the same reason T5 exists.
    if let Ok(mut guard) = state.supervisor.lock() {
        if let Some(mut old) = guard.take() {
            old.shutdown();
        }
    }

    let binary = resolve_binary(&app).ok_or("Could not find the momo-fetch binary.")?;
    let sup = Supervisor::start(&binary, &dir, &secrets::env_overrides())
        .map_err(|e| e.to_string())?;
    let url = sup.url.clone();

    if !wait_until_ready(&url, Duration::from_secs(30)).await {
        return Err("The gateway started but never became ready.".into());
    }

    if let Ok(mut guard) = state.supervisor.lock() {
        *guard = Some(sup);
    }
    if let Ok(mut guard) = state.url.lock() {
        *guard = Some(url.clone());
    }
    if let Ok(mut guard) = state.project.lock() {
        *guard = Some(dir.clone());
    }
    // Recorded here, after `await_ready`: a workspace that never came up is not
    // one to offer again at the top of the list.
    workspaces::record(&recents_store(&app), &dir);

    // The bundle reads the URL once at boot, so a re-rooted gateway needs a
    // reload rather than an event.
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.eval(format!(
            "window.__GATEWAY_URL__ = {}; window.location.reload();",
            serde_json::to_string(&url).unwrap_or_default()
        ));
    }

    Ok(url)
}

/// Reject a workspace name that is not a single new folder.
///
/// The name is joined onto a directory the user picked, so a separator or a
/// `..` would put the workspace somewhere they did not choose. Returned as a
/// message rather than sanitised silently: quietly turning `../x` into `x` is
/// its own surprise.
fn check_workspace_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Give the workspace a name.".into());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("A workspace name cannot contain a path separator.".into());
    }
    if name == "." || name == ".." {
        return Err("Give the workspace a name of its own.".into());
    }
    if name.starts_with('.') {
        return Err("A name starting with \".\" makes a hidden folder.".into());
    }
    Ok(name)
}

/// **T4b** — create a workspace under a directory the user picked, then open it.
///
/// Separate from [`open_project`] on purpose. "Open" and "create" answer
/// different questions, and a single button that seeded whatever folder it was
/// pointed at would turn a mis-click in the picker into a `.harness/` in
/// someone's home directory.
#[tauri::command]
async fn create_workspace(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    parent: String,
    name: String,
) -> Result<String, String> {
    let parent_dir = PathBuf::from(&parent);
    if !parent_dir.is_dir() {
        return Err(format!("{parent} is not a directory."));
    }

    let name = check_workspace_name(&name)?;
    let dir = parent_dir.join(name);

    if is_workspace(&dir) {
        return Err(format!(
            "{name} is already a workspace. Open it instead."
        ));
    }
    // An existing directory is fine to adopt — a checked-out repo is the normal
    // case — but only while it is not already someone else's workspace.
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create {name}: {e}"))?;

    let (provider, model) = inherited_selection(&state).await;
    seed_workspace(&dir, &provider, &model);

    if !is_workspace(&dir) {
        return Err(format!("Could not write settings into {name}."));
    }

    open_project(app, state, dir.display().to_string()).await
}

/// Where the gateway is currently rooted, for the `.env` overlap check.
///
/// The project someone opened, when they have opened one — falling back to the
/// default workspace only for a session that never left it. Reading the default
/// unconditionally, as this used to, meant that saving an API key restarted the
/// gateway *at a different project* than the one on screen.
fn current_workspace(app: &tauri::AppHandle) -> PathBuf {
    let state = app.state::<AppState>();
    if let Some(project) = state.project.lock().ok().and_then(|p| p.clone()) {
        return project;
    }
    default_project_dir(app).unwrap_or_else(|_| PathBuf::from("."))
}

/// Per-provider key status. **Never returns a key** — only whether one is set.
#[tauri::command]
fn secret_status(app: tauri::AppHandle) -> Vec<secrets::SecretStatus> {
    secrets::status(&current_workspace(&app))
}

/// Store a key and restart the gateway so it picks it up.
///
/// The harness resolves secrets while building, not per request, so a running
/// gateway would keep using the old value. Restarting is the honest way to make
/// "saved" mean "in effect".
#[tauri::command]
async fn set_secret(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    provider: String,
    key: String,
) -> Result<(), String> {
    secrets::set(&provider, &key)?;
    restart_gateway(&app, &state).await
}

#[tauri::command]
async fn delete_secret(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    provider: String,
) -> Result<(), String> {
    secrets::delete(&provider)?;
    restart_gateway(&app, &state).await
}

/// Stop the gateway and start a fresh one at the same project.
async fn restart_gateway(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
) -> Result<(), String> {
    let project = current_workspace(app);

    if let Ok(mut guard) = state.supervisor.lock() {
        if let Some(mut old) = guard.take() {
            old.shutdown();
        }
    }

    let binary = resolve_binary(app).ok_or("Could not find the momo-fetch binary.")?;
    let sup = Supervisor::start(&binary, &project, &secrets::env_overrides())
        .map_err(|e| e.to_string())?;
    let url = sup.url.clone();

    if !wait_until_ready(&url, Duration::from_secs(30)).await {
        return Err("The gateway restarted but never became ready.".into());
    }

    if let Ok(mut guard) = state.supervisor.lock() {
        *guard = Some(sup);
    }
    if let Ok(mut guard) = state.url.lock() {
        *guard = Some(url.clone());
    }
    if let Ok(mut guard) = state.startup_error.lock() {
        *guard = None;
    }

    if let Some(window) = app.get_webview_window("main") {
        let _ = window.eval(format!(
            "window.__GATEWAY_URL__ = {}; window.location.reload();",
            serde_json::to_string(&url).unwrap_or_default()
        ));
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    // **T5** — single instance.
    //
    // Not a convenience: the harness is process-global state, and two gateways
    // over one `.harness/` corrupt the session DB and race the cost file. Focus
    // the existing window instead of starting a second shell.
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
            // On Windows and Linux a deep link opened while the app is already
            // running arrives as argv on the *second* instance, which this
            // callback receives. Without handling it here, momo:// links only
            // work on a cold start.
            if let Some(url) = argv.iter().find(|a| a.starts_with("momo://")) {
                deeplink::dispatch(app, url);
            }
        }));
    }

    builder
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        // **T8** — remember size, position and maximised state.
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_log::Builder::default().build())
        .manage(AppState {
            supervisor: Mutex::new(None),
            project: Mutex::new(None),
            url: Mutex::new(None),
            startup_error: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            gateway_url,
            startup_error,
            gateway_stderr,
            open_project,
            create_workspace,
            recent_workspaces,
            secret_status,
            set_secret,
            delete_secret,
        ])
        .setup(|app| {
            // T9 — links delivered while the app is running (macOS) come
            // through here.
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let handle = app.handle().clone();
                app.deep_link().on_open_url(move |event| {
                    for url in event.urls() {
                        deeplink::dispatch(&handle, url.as_str());
                    }
                });
            }

            // Menus and the tray are best-effort: a tray icon failure must not
            // take down the app, and previously it would have — `?` here turned
            // any menu error into a failed setup.
            if let Err(e) = menu::install(app.handle()) {
                log::error!("menu/tray setup failed: {e}");
            }

            match app.get_webview_window("main") {
                Some(w) => {
                    log::info!("main window created; showing");
                    let _ = w.show();
                    let _ = w.set_focus();
                }
                None => log::error!("no webview window labelled 'main' was created"),
            }

            let handle = app.handle().clone();

            // Start the gateway on a background thread.
            //
            // This used to run inline here, and the window never appeared: the
            // event loop has not started during `setup`, so blocking for the
            // child's listening line plus a /health poll starves window
            // creation entirely. The app showed a menu bar and nothing else.
            //
            // Off-thread, the window appears immediately with the boot screen
            // and fills in when the gateway is ready — which is also the better
            // experience, since a cold start can take seconds.
            std::thread::spawn(move || {
                let state = handle.state::<AppState>();

                let project = match default_project_dir(&handle) {
                    Ok(p) => p,
                    Err(e) => return set_error(&state, e),
                };

                let Some(binary) = resolve_binary(&handle) else {
                    return set_error(
                        &state,
                        "Could not find the momo-fetch binary. Build it with \
                         `cargo build --release`."
                            .into(),
                    );
                };

                match Supervisor::start(&binary, &project, &secrets::env_overrides()) {
                    Ok(sup) => {
                        let url = sup.url.clone();
                        log::info!("gateway listening on {url}");

                        if gateway::await_ready(&url, Duration::from_secs(30)) {
                            if let Ok(mut guard) = state.url.lock() {
                                *guard = Some(url.clone());
                            }
                            if let Ok(mut guard) = state.supervisor.lock() {
                                *guard = Some(sup);
                            }
                            // The workspace this launch is rooted at — both so
                            // a key save restarts *here*, and so it is offered
                            // as a way back after switching away from it.
                            if let Ok(mut guard) = state.project.lock() {
                                *guard = Some(project.clone());
                            }
                            workspaces::record(&recents_store(&handle), &project);
                            inject_url(&handle, &url);
                        } else {
                            set_error(
                                &state,
                                "The gateway started but never became ready.".into(),
                            );
                        }
                    }
                    Err(e) => set_error(&state, explain(&e.to_string(), &project)),
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building MOMO WORK")
        // Shut the gateway down on exit.
        //
        // This was hooked to `WindowEvent::Destroyed` and leaked the child on
        // every quit: a Cmd-Q / Quit AppleEvent terminates the app without
        // reliably delivering that event, and `Drop` does not help either
        // because the process exits without unwinding. `RunEvent::Exit` is the
        // hook that actually fires on every exit path.
        //
        // A leaked gateway is not cosmetic — it holds the session DB and an
        // ephemeral port, and the next launch fights it.
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(mut guard) = state.supervisor.lock() {
                        if let Some(mut sup) = guard.take() {
                            log::info!("shutting down the gateway");
                            sup.shutdown();
                        }
                    }
                }
            }
        });
}

/// Add the one thing a first-run failure is always missing: *where* to fix it.
///
/// The harness reports "Secret not found for 'openrouter'. Set
/// OPENROUTER_API_KEY…" — correct, and useless in a GUI, because the user has
/// no idea which directory `.env` is read from. The shell does know, so it says.
fn explain(error: &str, project: &std::path::Path) -> String {
    if error.contains("Secret not found") || error.contains("API_KEY") {
        format!(
            "{error}\n\nThe desktop app reads .env from its workspace:\n\n               {}/.env\n\nCreate that file with one line, e.g.\n\n               OPENROUTER_API_KEY=sk-or-...\n\nthen reopen the app.",
            project.display()
        )
    } else {
        error.to_string()
    }
}

fn set_error(state: &State<'_, AppState>, message: String) {
    log::error!("{message}");
    if let Ok(mut guard) = state.startup_error.lock() {
        *guard = Some(message);
    }
}

/// **T3** — inject the URL so it is set before the bundle's scripts run.
fn inject_url(app: &tauri::AppHandle, url: &str) {
    log::info!("injecting gateway url into webview: {url}");
    if let Some(window) = app.get_webview_window("main") {
        let js = format!(
            "window.__GATEWAY_URL__ = {};",
            serde_json::to_string(url).unwrap_or_default()
        );
        let _ = window.eval(js);
    }
}

#[cfg(test)]
mod workspace_tests {
    use super::*;

    #[test]
    fn a_directory_is_a_workspace_only_with_settings() {
        let tmp = tempfile::tempdir().unwrap();

        // memory-vault alone is not it: the harness creates that on the way to
        // failing, so an aborted open leaves one behind.
        std::fs::create_dir_all(tmp.path().join("memory-vault")).unwrap();
        assert!(!is_workspace(tmp.path()));

        std::fs::create_dir_all(tmp.path().join(".harness")).unwrap();
        assert!(!is_workspace(tmp.path()), "an empty .harness is not enough");

        std::fs::write(tmp.path().join(".harness/settings.json"), "{}").unwrap();
        assert!(is_workspace(tmp.path()));
    }

    #[test]
    fn seeding_inherits_the_running_selection() {
        let tmp = tempfile::tempdir().unwrap();
        seed_workspace(tmp.path(), "zai", "glm-5.3");

        let settings_file = tmp.path().join(".harness/settings.json");
        let written = std::fs::read_to_string(settings_file).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&written).unwrap();

        assert_eq!(parsed["default_provider"], "zai");
        assert_eq!(parsed["default_model"], "glm-5.3");
        // Strict by default: a brand-new workspace should ask before it acts.
        assert_eq!(parsed["permission_mode"], "strict");
        // Never a key, whatever the running session had.
        assert!(!written.to_lowercase().contains("key"));
    }

    #[test]
    fn seeding_leaves_an_existing_workspace_alone() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".harness")).unwrap();
        std::fs::write(tmp.path().join(".harness/settings.json"), "{\"mine\":true}").unwrap();

        seed_workspace(tmp.path(), "zai", "glm-5.3");

        let settings_file = tmp.path().join(".harness/settings.json");
        let written = std::fs::read_to_string(settings_file).unwrap();
        assert_eq!(written, "{\"mine\":true}");
    }

    #[test]
    fn a_name_must_be_one_new_folder() {
        // The name is joined onto a directory the user picked; anything that
        // can climb out of it is refused rather than cleaned up.
        assert!(check_workspace_name("my-app").is_ok());
        assert_eq!(check_workspace_name("  my-app  ").unwrap(), "my-app");

        for bad in ["", "   ", "a/b", "a\\b", "..", ".", "../escape", ".hidden"] {
            let refused = check_workspace_name(bad).is_err();
            assert!(refused, "{bad:?} should be refused");
        }
    }
}
