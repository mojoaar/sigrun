mod commands;
mod host;
mod tray;

use tauri::Manager;

/// The native About panel's metadata. Pure, so its fields are unit-testable
/// without a Tauri runtime.
///
/// macOS paints only `name`, `version`, `short_version`, `copyright` and
/// `credits` (tauri 2.12.4 / muda 0.20, `platform_impl/macos`); `authors`,
/// `comments`, `license` and `website` are documented there as unsupported, so
/// they go unread. The site rides the copyright line, which AppKit paints
/// centred: the `credits` field is handed to AppKit as a plain attributed
/// string with no paragraph style, so a credits line comes out left-aligned
/// with no way to centre it from here.
pub fn about_metadata() -> tauri::menu::AboutMetadata<'static> {
    tauri::menu::AboutMetadata {
        name: Some("Sigrun".into()),
        version: Some(env!("CARGO_PKG_VERSION").into()),
        copyright: Some("Copyright © 2026 mojoaar · https://johansen.foo".into()),
        // `tauri dev` runs a bare binary, not an app bundle, so AppKit has no
        // app icon to fall back on and paints the generic executable one.
        // Handing it the app icon explicitly makes the panel match everywhere.
        icon: tauri::image::Image::from_bytes(include_bytes!("../icons/128x128@2x.png")).ok(),
        ..Default::default()
    }
}

/// The macOS menu strings Sigrun writes itself. Predefined items (Services,
/// Hide, Cut/Copy/Paste, Fullscreen) keep the strings macOS supplies in the
/// system language; the product name is not translated. Matches `tray::labels`
/// in shape and fallback.
pub struct MenuLabels {
    pub edit: String,
    pub view: String,
    pub window: String,
    pub quit: String,
}

pub fn menu_labels(language: &str) -> MenuLabels {
    match language {
        "da" => MenuLabels {
            edit: "Rediger".into(),
            view: "Vis".into(),
            window: "Vindue".into(),
            quit: "Afslut Sigrun".into(),
        },
        _ => MenuLabels {
            edit: "Edit".into(),
            view: "View".into(),
            window: "Window".into(),
            quit: "Quit Sigrun".into(),
        },
    }
}

/// The window's native appearance for a Light/Dark/System setting. `None`
/// means "follow the OS", which is what `"system"` — and any unknown value —
/// asks for.
///
/// macOS paints the overlay title bar's traffic lights from the window's own
/// appearance rather than the webview's CSS, so the glyphs only stay legible
/// against the themed header if this tracks the app's mode.
pub fn window_theme(setting: &str) -> Option<tauri::Theme> {
    match setting {
        "dark" => Some(tauri::Theme::Dark),
        "light" => Some(tauri::Theme::Light),
        _ => None,
    }
}

/// Build and install the macOS app menu. Setting a menu replaces Tauri's
/// default one, so the Edit submenu (copy/paste in the app's forms) and the
/// Window submenu have to come with it, and the Quit item has to be a custom
/// one so it runs the tray's shutdown rather than a bare exit.
///
/// That custom item needs no handler of its own: Tauri dispatches every menu
/// event to every global menu listener (app.rs, `MenuEvent` arm), and the tray
/// registers one for [`tray::MENU_QUIT_ID`].
#[cfg(target_os = "macos")]
pub(crate) fn setup_macos_menu(handle: &tauri::AppHandle, language: &str) -> tauri::Result<()> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};

    let labels = menu_labels(language);
    let quit = MenuItemBuilder::with_id(tray::MENU_QUIT_ID, &labels.quit)
        // The predefined Quit would have carried ⌘Q; a custom one must ask.
        .accelerator("CmdOrCtrl+Q")
        .build(handle)?;
    let app_menu = SubmenuBuilder::new(handle, "Sigrun")
        .about(Some(about_metadata()))
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .item(&quit)
        .build()?;
    let edit_menu = SubmenuBuilder::new(handle, &labels.edit)
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let window_menu =
        SubmenuBuilder::with_id(handle, tauri::menu::WINDOW_SUBMENU_ID, &labels.window)
            .minimize()
            .maximize()
            .separator()
            .close_window()
            .build()?;
    let view_menu = SubmenuBuilder::new(handle, &labels.view)
        .fullscreen()
        .build()?;
    let menu = MenuBuilder::new(handle)
        .items(&[&app_menu, &edit_menu, &view_menu, &window_menu])
        .build()?;
    handle.set_menu(menu)?;
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::vault_init,
            commands::vault_unlock,
            commands::vault_lock,
            commands::list_connections,
            commands::add_connection,
            commands::update_connection,
            commands::remove_connection,
            commands::set_connection_password,
            commands::connect,
            commands::provision_connection,
            commands::trust_host,
            commands::list_keys,
            commands::generate_key,
            commands::import_key,
            commands::remove_key,
            commands::get_settings,
            commands::save_settings,
            commands::set_window_theme,
            commands::list_terminals,
            commands::open_external,
            commands::export_bundle,
            commands::import_bundle,
        ])
        .setup(|app| {
            let paths = sigrun_core::paths::Paths::resolve()?;
            let state = tauri::async_runtime::block_on(host::start_host(paths))?;
            app.manage(state.clone());
            // macOS gets an About that carries the author and the links. Windows
            // and Linux keep Tauri's default menu.
            #[cfg(target_os = "macos")]
            {
                let language = app
                    .state::<std::sync::Arc<sigrun_core::host::HostState>>()
                    .store
                    .load_settings()
                    .map(|s| s.language)
                    .unwrap_or_else(|_| "en".to_string());
                setup_macos_menu(app.handle(), &language)?;
            }
            if let Some(window) = app.get_webview_window("main") {
                let w = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = w.hide();
                    }
                });
            }
            // Start minimized to the tray when the user asked for it. Read
            // before `state` is moved into the tray; on any load error, show
            // the window (never strand a user with no window).
            let start_minimized = state
                .store
                .load_settings()
                .map(|s| s.start_minimized)
                .unwrap_or(false);
            if start_minimized {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
            // Tell the UI when the idle auto-lock fires, so it re-gates, and keep
            // the tray menu in step with the vault state (the CLI can change it).
            {
                use tauri::Emitter;
                let handle = app.handle().clone();
                let st = state.clone();
                tauri::async_runtime::spawn(async move {
                    let mut last_unlocked = None;
                    // `active_ids` is sorted, so `!=` is a correct change test.
                    let mut last_active = st.active_ids();
                    loop {
                        if st.shutdown.is_cancelled() {
                            return;
                        }
                        // A killed terminal group runs no handler, so reap the
                        // watched shells before the snapshot below: a dead one
                        // then clears the dot on this tick, not the next.
                        st.sweep_dead_sessions();
                        if st
                            .auto_locked
                            .swap(false, std::sync::atomic::Ordering::Relaxed)
                        {
                            let _ = handle.emit("vault-locked", ());
                        }
                        let unlocked = tray::is_unlocked(&st).await;
                        if last_unlocked != Some(unlocked) {
                            // Only remember the state once the tray really has it,
                            // so a failed sync is retried on the next tick.
                            match tray::sync_tray_now(&handle, &st).await {
                                Ok(()) => last_unlocked = Some(unlocked),
                                Err(e) => eprintln!("sigrun: could not update the tray: {e}"),
                            }
                        }
                        // Tell the UI when a connection gained or lost its live
                        // terminal (a vault lock clears the set too).
                        let active = st.active_ids();
                        if last_active != active {
                            let _ = handle.emit("connections-changed", ());
                            last_active = active;
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                });
            }
            tray::build_tray(app.handle(), state)?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Sigrun")
        .run(|app_handle, event| {
            // Clicking the dock icon with no visible window reopens it on macOS.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                tray::show_main(app_handle);
            }
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                if let Some(state) =
                    app_handle.try_state::<std::sync::Arc<sigrun_core::host::HostState>>()
                {
                    state.shutdown.cancel();
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_about_metadata_names_the_project_and_its_copyright() {
        let meta = super::about_metadata();
        assert_eq!(meta.name.as_deref(), Some("Sigrun"));
        assert_eq!(meta.version.as_deref(), Some(env!("CARGO_PKG_VERSION")));
        assert_eq!(
            meta.copyright.as_deref(),
            Some("Copyright © 2026 mojoaar · https://johansen.foo")
        );
        // macOS paints no other field: a separate `credits` line repeating the
        // author, the website and the licence was removed, and no field AppKit
        // ignores is set.
        assert_eq!(meta.credits, None);
        assert_eq!(meta.authors, None);
        assert_eq!(meta.website, None);
        assert_eq!(meta.license, None);
        let icon = meta
            .icon
            .expect("the about panel should carry the app icon");
        assert_eq!((icon.width(), icon.height()), (256, 256));
    }

    #[test]
    fn the_window_theme_follows_the_setting() {
        assert_eq!(window_theme("dark"), Some(tauri::Theme::Dark));
        assert_eq!(window_theme("light"), Some(tauri::Theme::Light));
        assert_eq!(window_theme("system"), None);
        assert_eq!(window_theme("nordlys"), None);
    }

    #[test]
    fn window_config() {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(v["productName"], "Sigrun");
        assert_eq!(v["identifier"], "com.sigrun.app");
        assert_eq!(v["app"]["windows"][0]["title"], "Sigrun");
        assert_eq!(v["app"]["windows"][0]["minWidth"], 900);
        // The installer metadata and the in-app tagline are the same approved
        // sentence, pinned here so the three copy surfaces cannot drift.
        assert_eq!(
            v["bundle"]["shortDescription"],
            "SSH connections and keys in a local encrypted vault, served to ssh through a built-in agent."
        );
        // The overlay title bar is what makes the header the visible bar; pin it
        // so a config edit cannot silently drop the feature.
        assert_eq!(v["app"]["windows"][0]["titleBarStyle"], "Overlay");
        assert_eq!(v["app"]["windows"][0]["hiddenTitle"], true);
    }
}

#[cfg(test)]
mod menu_tests {
    use super::menu_labels;

    #[test]
    fn menu_labels_are_localized() {
        let en = menu_labels("en");
        assert_eq!(
            (
                en.edit.as_str(),
                en.view.as_str(),
                en.window.as_str(),
                en.quit.as_str()
            ),
            ("Edit", "View", "Window", "Quit Sigrun")
        );
        let da = menu_labels("da");
        assert_eq!(
            (
                da.edit.as_str(),
                da.view.as_str(),
                da.window.as_str(),
                da.quit.as_str()
            ),
            ("Rediger", "Vis", "Vindue", "Afslut Sigrun")
        );
        // An unknown language falls back to English, like the tray.
        assert_eq!(menu_labels("kl").edit, "Edit");
    }
}
