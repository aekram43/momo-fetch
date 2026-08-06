//! **T6** system tray and **T7** native menus.
//!
//! Both drive the same actions as the web UI's keyboard shortcuts (F20), so a
//! user who learned one has learned the other.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    install_menu(app)?;
    install_tray(app)?;
    Ok(())
}

/// Native menu bar.
///
/// Menu items emit events the web UI listens for, rather than reaching into it
/// directly — the same action then has one implementation whether it came from
/// a menu, the tray, or a keystroke.
fn install_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let new_session = MenuItem::with_id(app, "new-session", "New Session", true, Some("CmdOrCtrl+N"))?;
    let open_project = MenuItem::with_id(app, "open-project", "Open Project…", true, Some("CmdOrCtrl+O"))?;
    let toggle_sidebar = MenuItem::with_id(app, "toggle-sidebar", "Toggle Sessions Panel", true, Some("CmdOrCtrl+B"))?;
    let toggle_detail = MenuItem::with_id(app, "toggle-detail", "Toggle Detail Panel", true, Some("CmdOrCtrl+J"))?;
    let interrupt = MenuItem::with_id(app, "interrupt", "Interrupt Turn", true, Some("Escape"))?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?;

    let file = Submenu::with_items(
        app,
        "File",
        true,
        &[
            &new_session,
            &open_project,
            &PredefinedMenuItem::separator(app)?,
            // Settings belongs in the app menu on macOS, but Tauri's app menu
            // is built by the platform; File is where it is reachable without
            // rebuilding that. The Cmd+, accelerator is the part users use.
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;

    // Edit needs the predefined items or the webview loses clipboard and undo
    // on macOS — they are wired to the native responder chain, not to us.
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;

    let view = Submenu::with_items(
        app,
        "View",
        true,
        &[&toggle_sidebar, &toggle_detail, &PredefinedMenuItem::separator(app)?, &interrupt],
    )?;

    let menu = Menu::with_items(app, &[&file, &edit, &view])?;
    app.set_menu(menu)?;

    app.on_menu_event(|app, event| {
        let id = event.id().0.as_str();
        if let Some(window) = app.get_webview_window("main") {
            // One channel for every menu action. The UI decides what each means.
            let _ = window.eval(format!(
                "window.dispatchEvent(new CustomEvent('momo:menu', {{ detail: {} }}))",
                serde_json::to_string(id).unwrap_or_else(|_| "\"\"".into())
            ));
        }
    });

    Ok(())
}

/// Tray icon: minimise-to-tray plus the two actions worth reaching without the
/// window.
fn install_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "tray-show", "Show Window", true, None::<&str>)?;
    let new_session = MenuItem::with_id(app, "new-session", "New Session", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "tray-quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &new_session, &quit])?;

    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().cloned().ok_or_else(|| {
            tauri::Error::AssetNotFound("default window icon".into())
        })?)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().0.as_str() {
            "tray-show" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.unminimize();
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            // Exit through the normal path so the window `Destroyed` handler
            // runs and the gateway is shut down rather than orphaned.
            "tray-quit" => app.exit(0),
            id => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                    let _ = w.eval(format!(
                        "window.dispatchEvent(new CustomEvent('momo:menu', {{ detail: {} }}))",
                        serde_json::to_string(id).unwrap_or_else(|_| "\"\"".into())
                    ));
                }
            }
        })
        .on_tray_icon_event(|tray, event| {
            // Left click raises the window — the behaviour people expect from a
            // tray icon, and the reason `show_menu_on_left_click` is off.
            if let TrayIconEvent::Click { button, .. } = event {
                if button == tauri::tray::MouseButton::Left {
                    if let Some(w) = tray.app_handle().get_webview_window("main") {
                        let _ = w.unminimize();
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
            }
        })
        .build(app)?;

    Ok(())
}
