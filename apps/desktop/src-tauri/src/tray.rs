use std::sync::Arc;

use sigrun_core::host::HostState;
use sigrun_core::ipc::protocol::Response;
use sigrun_core::model::Connection;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

const NORMAL_TITLE: &str = "Sigrun";
const ATTENTION_TITLE: &str = "Sigrun — connection needs attention";
const TRAY_ID: &str = "sigrun-tray";

pub fn tray_menu_model(connections: &[Connection]) -> Vec<(String, String)> {
    connections
        .iter()
        .map(|c| (format!("conn:{}", c.id), c.label.clone()))
        .chain([
            ("open".to_string(), "Open Sigrun".to_string()),
            ("quit".to_string(), "Quit".to_string()),
        ])
        .collect()
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn set_title(app: &AppHandle, title: &str) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_title(title);
    }
}

fn build_menu(app: &AppHandle, conns: &[Connection]) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;
    let sep = PredefinedMenuItem::separator(app)?;
    for (id, label) in tray_menu_model(conns) {
        if id == "open" {
            menu.append(&sep)?;
        }
        menu.append(&MenuItem::with_id(app, id, label, true, None::<&str>)?)?;
    }
    Ok(menu)
}

async fn load_connections(state: &Arc<HostState>) -> Vec<Connection> {
    match sigrun_core::host::handle(state, sigrun_core::ipc::protocol::Request::ConnectionsList)
        .await
    {
        Response::Connections(c) => c,
        _ => Vec::new(),
    }
}

fn refresh(app: &AppHandle, state: &Arc<HostState>) {
    let (app, state) = (app.clone(), state.clone());
    tauri::async_runtime::spawn(async move {
        let conns = load_connections(&state).await;
        if let (Some(tray), Ok(menu)) = (app.tray_by_id(TRAY_ID), build_menu(&app, &conns)) {
            let _ = tray.set_menu(Some(menu));
        }
    });
}

const TRAY_TEMPLATE: &[u8] = include_bytes!("../icons/tray-template.png");
const TRAY_LIGHT: &[u8] = include_bytes!("../icons/tray-light.png");

pub fn build_tray(app: &AppHandle, state: Arc<HostState>) -> tauri::Result<()> {
    let menu = build_menu(app, &[])?;
    let st = state.clone();
    // macOS recolours a template icon white/dark to match the menu bar; the
    // light variant is the fallback for trays without template recolouring.
    let tray_icon = if cfg!(target_os = "macos") {
        tauri::image::Image::from_bytes(TRAY_TEMPLATE)?
    } else {
        tauri::image::Image::from_bytes(TRAY_LIGHT)?
    };
    let is_template = cfg!(target_os = "macos");
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(tray_icon)
        .icon_as_template(is_template)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            // Left-click toggles the window: show+focus if hidden, hide if visible.
            use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(w) = app.get_webview_window("main") {
                    match (w.is_visible(), w.is_minimized()) {
                        (Ok(true), Ok(false)) => {
                            let _ = w.hide();
                        }
                        _ => show_main(app),
                    }
                }
            }
        })
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "quit" => {
                st.shutdown.cancel();
                app.exit(0);
            }
            id => {
                if let Some(cid) = id.strip_prefix("conn:") {
                    let (app, st, cid) = (app.clone(), st.clone(), cid.to_string());
                    tauri::async_runtime::spawn(async move {
                        // Unknown/mismatched keys and any other error surface in the window.
                        let title = match crate::commands::connect_guarded(st, cid, false).await {
                            Ok(_) => NORMAL_TITLE,
                            Err(e) => {
                                show_main(&app);
                                if e.code == "vault_locked" {
                                    let _ = app.emit("vault-locked", ());
                                }
                                if e.code.starts_with("host_key_") {
                                    ATTENTION_TITLE
                                } else {
                                    NORMAL_TITLE
                                }
                            }
                        };
                        set_title(&app, title);
                    });
                }
            }
        })
        .build(app)?;
    refresh(app, &state);
    if let Some(w) = app.get_webview_window("main") {
        let (handle, st) = (app.clone(), state);
        w.on_window_event(move |e| {
            if matches!(e, WindowEvent::Focused(true)) {
                set_title(&handle, NORMAL_TITLE);
                refresh(&handle, &st);
            }
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_model_order_and_prefix() {
        let c = |id: &str, label: &str| Connection {
            id: id.into(),
            label: label.into(),
            host: String::new(),
            user: String::new(),
            port: 22,
            key_id: None,
            group: None,
            tags: Vec::new(),
            source: sigrun_core::model::ConnectionSource::Managed,
        };
        let m = tray_menu_model(&[c("a", "A"), c("b", "B")]);
        assert_eq!(
            m,
            vec![
                ("conn:a".into(), "A".into()),
                ("conn:b".into(), "B".into()),
                ("open".into(), "Open Sigrun".into()),
                ("quit".into(), "Quit".into()),
            ]
        );
    }
}
