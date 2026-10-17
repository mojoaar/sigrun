use std::sync::Arc;

use sigrun_core::host::HostState;
use sigrun_core::ipc::protocol::{Request, Response};
use sigrun_core::model::{Connection, ConnectionSource};
use sigrun_core::vault::VaultStatus;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

const NORMAL_TITLE: &str = "Sigrun";
const ATTENTION_TITLE: &str = "Sigrun — connection needs attention";
const TRAY_ID: &str = "sigrun-tray";

/// The tray strings Sigrun writes itself. Connection labels and group names are
/// user data and are never translated.
#[derive(Debug, Clone, PartialEq)]
pub struct TrayLabels {
    pub open: String,
    pub quit: String,
    pub unlock: String,
    pub discovered: String,
}

pub fn labels(language: &str) -> TrayLabels {
    match language {
        "da" => TrayLabels {
            open: "Åbn Sigrun".into(),
            quit: "Afslut".into(),
            unlock: "Lås Sigrun op…".into(),
            discovered: "Fundet automatisk".into(),
        },
        _ => TrayLabels {
            open: "Open Sigrun".into(),
            quit: "Quit".into(),
            unlock: "Unlock Sigrun…".into(),
            discovered: "Discovered".into(),
        },
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrayEntry {
    Item {
        id: String,
        label: String,
    },
    Submenu {
        label: String,
        items: Vec<(String, String)>,
    },
    Separator,
}

/// The tray menu as data, so it can be tested without a Tauri runtime.
///
/// Ungrouped connections come first, then one submenu per group, then the hosts
/// discovered in `~/.ssh/config`. A locked vault offers no connections: there is
/// nothing to connect to. Linux never delivers tray click events, so its locked
/// menu leads with the unlock item instead of relying on the click-to-show path.
pub fn tray_menu_model(
    conns: &[Connection],
    unlocked: bool,
    labels: &TrayLabels,
    linux: bool,
) -> Vec<TrayEntry> {
    let mut out = Vec::new();
    if unlocked {
        let mut loose = Vec::new();
        let mut discovered = Vec::new();
        let mut groups: std::collections::BTreeMap<String, Vec<&Connection>> = Default::default();
        for c in conns {
            if c.source == ConnectionSource::Discovered {
                discovered.push(c);
                continue;
            }
            match c.group.as_deref().map(str::trim).filter(|g| !g.is_empty()) {
                Some(g) => groups.entry(g.to_string()).or_default().push(c),
                None => loose.push(c),
            }
        }
        let by_label = |v: &mut Vec<&Connection>| v.sort_by(|a, b| a.label.cmp(&b.label));
        by_label(&mut loose);
        by_label(&mut discovered);
        for c in loose {
            out.push(TrayEntry::Item {
                id: connection_id(c),
                label: c.label.clone(),
            });
        }
        if !out.is_empty() && (!groups.is_empty() || !discovered.is_empty()) {
            out.push(TrayEntry::Separator);
        }
        for (group, mut members) in groups {
            by_label(&mut members);
            out.push(TrayEntry::Submenu {
                label: group,
                items: members
                    .into_iter()
                    .map(|c| (connection_id(c), c.label.clone()))
                    .collect(),
            });
        }
        if !discovered.is_empty() {
            out.push(TrayEntry::Submenu {
                label: labels.discovered.clone(),
                items: discovered
                    .into_iter()
                    .map(|c| (connection_id(c), c.label.clone()))
                    .collect(),
            });
        }
        if !out.is_empty() {
            out.push(TrayEntry::Separator);
        }
    }
    if !unlocked && linux {
        out.push(TrayEntry::Item {
            id: "unlock".into(),
            label: labels.unlock.clone(),
        });
    } else {
        out.push(TrayEntry::Item {
            id: "open".into(),
            label: labels.open.clone(),
        });
    }
    out.push(TrayEntry::Item {
        id: "quit".into(),
        label: labels.quit.clone(),
    });
    out
}

fn connection_id(c: &Connection) -> String {
    format!("conn:{}", c.id)
}

fn build_menu(app: &AppHandle, entries: &[TrayEntry]) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;
    for entry in entries {
        match entry {
            TrayEntry::Separator => menu.append(&PredefinedMenuItem::separator(app)?)?,
            TrayEntry::Item { id, label } => {
                menu.append(&MenuItem::with_id(app, id, label, true, None::<&str>)?)?
            }
            TrayEntry::Submenu { label, items } => {
                let submenu = Submenu::new(app, label, true)?;
                for (id, item_label) in items {
                    submenu.append(&MenuItem::with_id(app, id, item_label, true, None::<&str>)?)?;
                }
                menu.append(&submenu)?;
            }
        }
    }
    Ok(menu)
}

pub async fn is_unlocked(state: &Arc<HostState>) -> bool {
    matches!(
        sigrun_core::host::handle(state, Request::Status).await,
        Response::Status(info) if info.vault == VaultStatus::Unlocked
    )
}

async fn load_connections(state: &Arc<HostState>) -> Vec<Connection> {
    match sigrun_core::host::handle(state, Request::ConnectionsList).await {
        Response::Connections(c) => c,
        _ => Vec::new(),
    }
}

/// Rebuild the tray menu and left-click behaviour for the current vault state
/// and language. Spawned, because every caller is either sync or already async.
pub fn sync_tray(app: &AppHandle, state: &Arc<HostState>) {
    let (app, state) = (app.clone(), state.clone());
    tauri::async_runtime::spawn(async move {
        if let Err(e) = sync_tray_now(&app, &state).await {
            eprintln!("sigrun: could not update the tray: {e}");
        }
    });
}

pub async fn sync_tray_now(app: &AppHandle, state: &Arc<HostState>) -> tauri::Result<()> {
    let unlocked = is_unlocked(state).await;
    let language = state
        .store
        .load_settings()
        .map(|s| s.language)
        .unwrap_or_else(|_| "en".to_string());
    let conns = if unlocked {
        load_connections(state).await
    } else {
        Vec::new()
    };
    let entries = tray_menu_model(
        &conns,
        unlocked,
        &labels(&language),
        cfg!(target_os = "linux"),
    );
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(build_menu(app, &entries)?))?;
        // Unlocked, the OS opens the attached menu on left click; locked, left
        // click has to reach our handler so it can show the window.
        let _ = tray.set_show_menu_on_left_click(unlocked);
    }
    Ok(())
}

pub(crate) fn show_main(app: &AppHandle) {
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

const TRAY_TEMPLATE: &[u8] = include_bytes!("../icons/tray-template.png");
const TRAY_LIGHT: &[u8] = include_bytes!("../icons/tray-light.png");

pub fn build_tray(app: &AppHandle, state: Arc<HostState>) -> tauri::Result<()> {
    let st = state.clone();
    // macOS recolours a template icon white/dark to match the menu bar; the
    // light variant is the fallback for trays without template recolouring.
    let tray_icon = if cfg!(target_os = "macos") {
        tauri::image::Image::from_bytes(TRAY_TEMPLATE)?
    } else {
        tauri::image::Image::from_bytes(TRAY_LIGHT)?
    };
    let is_template = cfg!(target_os = "macos");
    // Attach a locked-shape menu up front: the async sync fills in the real one,
    // and on Linux the icon is not always shown until a menu is set.
    let language = state
        .store
        .load_settings()
        .map(|s| s.language)
        .unwrap_or_else(|_| "en".to_string());
    let initial = tray_menu_model(&[], false, &labels(&language), cfg!(target_os = "linux"));
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(tray_icon)
        .icon_as_template(is_template)
        .menu(&build_menu(app, &initial)?)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle().clone();
                let Some(state) = app.try_state::<Arc<HostState>>().map(|s| s.inner().clone())
                else {
                    return;
                };
                tauri::async_runtime::spawn(async move {
                    // Unlocked, the OS already opened the menu (the flag is on),
                    // so this only fires in the locked case.
                    if !is_unlocked(&state).await {
                        show_main(&app);
                        let _ = app.emit("vault-locked", ());
                    }
                });
            }
        })
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" | "unlock" => show_main(app),
            "quit" => {
                st.shutdown.cancel();
                app.exit(0);
            }
            id => {
                if let Some(cid) = id.strip_prefix("conn:") {
                    let (app, st, cid) = (app.clone(), st.clone(), cid.to_string());
                    tauri::async_runtime::spawn(async move {
                        // Unknown/mismatched keys and any other error surface in the window.
                        let title =
                            match crate::commands::connect_guarded(st.clone(), cid, false, false)
                                .await
                            {
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
                        sync_tray(&app, &st);
                    });
                }
            }
        })
        .build(app)?;
    sync_tray(app, &state);
    if let Some(w) = app.get_webview_window("main") {
        let (handle, st) = (app.clone(), state);
        w.on_window_event(move |e| {
            if matches!(e, WindowEvent::Focused(true)) {
                set_title(&handle, NORMAL_TITLE);
                sync_tray(&handle, &st);
            }
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sigrun_core::model::ConnectionSource;

    fn conn(id: &str, label: &str, group: Option<&str>, source: ConnectionSource) -> Connection {
        Connection {
            id: id.into(),
            label: label.into(),
            host: String::new(),
            user: String::new(),
            port: 22,
            key_id: None,
            group: group.map(str::to_string),
            tags: Vec::new(),
            source,
            disable_password_auth: false,
        }
    }

    fn item(id: &str, label: &str) -> TrayEntry {
        TrayEntry::Item {
            id: id.into(),
            label: label.into(),
        }
    }

    fn submenu(label: &str, items: &[(&str, &str)]) -> TrayEntry {
        TrayEntry::Submenu {
            label: label.into(),
            items: items
                .iter()
                .map(|(id, l)| ((*id).to_string(), (*l).to_string()))
                .collect(),
        }
    }

    #[test]
    fn groups_discovered_and_loose_connections() {
        let en = labels("en");
        let conns = vec![
            conn("1", "prod-web", Some("Work"), ConnectionSource::Managed),
            conn("2", "nas", None, ConnectionSource::Managed),
            conn("3", "github.com", None, ConnectionSource::Discovered),
            conn("4", "api", Some("Work"), ConnectionSource::Managed),
            conn("5", "air", None, ConnectionSource::Managed),
        ];
        assert_eq!(
            tray_menu_model(&conns, true, &en, false),
            vec![
                item("conn:5", "air"),
                item("conn:2", "nas"),
                TrayEntry::Separator,
                submenu("Work", &[("conn:4", "api"), ("conn:1", "prod-web")]),
                submenu("Discovered", &[("conn:3", "github.com")]),
                TrayEntry::Separator,
                item("open", "Open Sigrun"),
                item("quit", "Quit"),
            ]
        );
    }

    #[test]
    fn a_locked_vault_offers_no_connections() {
        let en = labels("en");
        let conns = vec![conn(
            "1",
            "prod-web",
            Some("Work"),
            ConnectionSource::Managed,
        )];
        assert_eq!(
            tray_menu_model(&conns, false, &en, false),
            vec![item("open", "Open Sigrun"), item("quit", "Quit")]
        );
        // Linux cannot deliver left clicks, so the locked menu offers the unlock.
        assert_eq!(
            tray_menu_model(&conns, false, &en, true),
            vec![item("unlock", "Unlock Sigrun…"), item("quit", "Quit")]
        );
    }

    #[test]
    fn an_empty_vault_has_no_leading_separator() {
        let en = labels("en");
        assert_eq!(
            tray_menu_model(&[], true, &en, false),
            vec![item("open", "Open Sigrun"), item("quit", "Quit")]
        );
    }

    #[test]
    fn blank_group_names_count_as_ungrouped() {
        let en = labels("en");
        let conns = vec![conn("1", "web", Some("   "), ConnectionSource::Managed)];
        assert_eq!(
            tray_menu_model(&conns, true, &en, false),
            vec![
                item("conn:1", "web"),
                TrayEntry::Separator,
                item("open", "Open Sigrun"),
                item("quit", "Quit"),
            ]
        );
    }

    #[test]
    fn labels_are_localized() {
        assert_eq!(labels("da").quit, "Afslut");
        assert_eq!(labels("da").open, "Åbn Sigrun");
        assert_eq!(labels("da").unlock, "Lås Sigrun op…");
        assert_eq!(labels("da").discovered, "Fundet automatisk");
        // An unknown language falls back to English.
        assert_eq!(labels("de"), labels("en"));
    }

    /// The tray strings live in Rust and in the app's translation files. This
    /// keeps the two tables from drifting apart.
    #[test]
    fn tray_labels_match_the_translation_files() {
        for (language, json) in [
            ("en", include_str!("../../src/i18n/en.json")),
            ("da", include_str!("../../src/i18n/da.json")),
        ] {
            let v: serde_json::Value = serde_json::from_str(json).unwrap();
            let tray = &v["tray"];
            let l = labels(language);
            assert_eq!(tray["open"], l.open, "{language} tray.open");
            assert_eq!(tray["quit"], l.quit, "{language} tray.quit");
            assert_eq!(tray["unlock"], l.unlock, "{language} tray.unlock");
            assert_eq!(
                tray["discovered"], l.discovered,
                "{language} tray.discovered"
            );
        }
    }
}
