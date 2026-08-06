//! MoMo Worker desktop shell.
//!
//! Wraps the exported web UI and supervises a `momo-fetch --gateway` child.
//! The web UI is byte-identical to the browser build — everything
//! desktop-specific lives here.

mod deeplink;
mod gateway;
mod menu;

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use tauri::{Manager, State};

use gateway::Supervisor;

/// Everything the shell needs to answer the frontend and shut down cleanly.
pub struct AppState {
    pub supervisor: Mutex<Option<Supervisor>>,
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
    seed_workspace(&workspace);
    Ok(workspace)
}

/// Give a fresh workspace enough config to start.
///
/// Without a `settings.json` the harness falls back to `anthropic` and dies with
/// "Secret not found for 'anthropic'" — a confusing first run when the user may
/// have configured a different provider entirely. Seeding a default at least
/// makes the failure name the provider it will actually use.
///
/// **Only non-secret config is written.** The API key is never seeded, guessed
/// or copied from elsewhere on disk; the user supplies it, and the boot screen
/// says how.
fn seed_workspace(dir: &std::path::Path) {
    let harness = dir.join(".harness");
    if harness.join("settings.json").exists() {
        return;
    }
    if std::fs::create_dir_all(&harness).is_err() {
        return;
    }
    const DEFAULTS: &str = r#"{
  "permission_mode": "strict",
  "default_provider": "openrouter",
  "default_model": "nvidia/nemotron-3-ultra-550b-a55b:free"
}
"#;
    let _ = std::fs::write(harness.join("settings.json"), DEFAULTS);

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

    // Stop the old gateway *before* starting a new one: two gateways over one
    // `.harness/` fight over the session DB — the same reason T5 exists.
    if let Ok(mut guard) = state.supervisor.lock() {
        if let Some(mut old) = guard.take() {
            old.shutdown();
        }
    }

    let binary = resolve_binary(&app).ok_or("Could not find the momo-fetch binary.")?;
    let sup = Supervisor::start(&binary, &dir).map_err(|e| e.to_string())?;
    let url = sup.url.clone();

    if !gateway::await_ready(&url, Duration::from_secs(30)) {
        return Err("The gateway started but never became ready.".into());
    }

    if let Ok(mut guard) = state.supervisor.lock() {
        *guard = Some(sup);
    }
    if let Ok(mut guard) = state.url.lock() {
        *guard = Some(url.clone());
    }

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
            url: Mutex::new(None),
            startup_error: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            gateway_url,
            startup_error,
            gateway_stderr,
            open_project,
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

                match Supervisor::start(&binary, &project) {
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
                            inject_url(&handle, &url);
                        } else {
                            set_error(
                                &state,
                                "The gateway started but never became ready.".into(),
                            );
                        }
                    }
                    Err(e) => set_error(&state, e.to_string()),
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building MoMo Worker")
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
