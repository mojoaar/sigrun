mod commands;
mod host;
mod tray;

use tauri::Manager;

/// The native About panel's metadata. Pure, so its fields are unit-testable
/// without a Tauri runtime.
///
/// macOS renders only `name`, `version`, `short_version`, `copyright` and
/// `credits` (tauri 2.12.4 / muda 0.20, `platform_impl/macos`); `authors`,
/// `comments`, `license` and `website` are documented there as unsupported, and
/// Tauri passes them through without folding them into `credits`. The `credits`
/// line is therefore what puts the author, the website and the licence in front
/// of a macOS user; the other fields go unread, and are kept so the struct says
/// what the project is rather than what this one platform happens to paint.
pub fn about_metadata() -> tauri::menu::AboutMetadata<'static> {
    tauri::menu::AboutMetadata {
        name: Some("Sigrun".into()),
        version: Some(env!("CARGO_PKG_VERSION").into()),
        authors: Some(vec!["mojoaar".into()]),
        // Mirrors `shortDescription` in tauri.conf.json, which macOS ignores.
        comments: Some("SSH key and connection manager".into()),
        copyright: Some("Copyright © 2026 mojoaar".into()),
        license: Some(env!("CARGO_PKG_LICENSE").into()),
        website: Some("https://johansen.foo".into()),
        credits: Some(concat!("mojoaar\nhttps://johansen.foo\n", env!("CARGO_PKG_LICENSE")).into()),
        ..Default::default()
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
fn setup_macos_menu(app: &tauri::App) -> tauri::Result<()> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};

    let handle = app.handle();
    let quit = MenuItemBuilder::with_id(tray::MENU_QUIT_ID, "Quit Sigrun")
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
    let edit_menu = SubmenuBuilder::new(handle, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let window_menu = SubmenuBuilder::with_id(handle, tauri::menu::WINDOW_SUBMENU_ID, "Window")
        .minimize()
        .maximize()
        .separator()
        .close_window()
        .build()?;
    let view_menu = SubmenuBuilder::new(handle, "View").fullscreen().build()?;
    let menu = MenuBuilder::new(handle)
        .items(&[&app_menu, &edit_menu, &view_menu, &window_menu])
        .build()?;
    app.set_menu(menu)?;
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
            commands::list_terminals,
            commands::open_external,
        ])
        .setup(|app| {
            let paths = sigrun_core::paths::Paths::resolve()?;
            let state = tauri::async_runtime::block_on(host::start_host(paths))?;
            app.manage(state.clone());
            // macOS gets an About that carries the author and the links. Windows
            // and Linux keep Tauri's default menu.
            #[cfg(target_os = "macos")]
            setup_macos_menu(app)?;
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
    #[test]
    fn the_about_metadata_names_the_project_its_author_and_its_licence() {
        let meta = super::about_metadata();
        assert_eq!(meta.name.as_deref(), Some("Sigrun"));
        assert_eq!(meta.version.as_deref(), Some(env!("CARGO_PKG_VERSION")));
        assert_eq!(
            meta.authors.as_deref(),
            Some(["mojoaar".to_string()].as_slice())
        );
        assert_eq!(meta.website.as_deref(), Some("https://johansen.foo"));
        assert_eq!(meta.license.as_deref(), Some("MIT"));
        assert_eq!(
            meta.credits.as_deref(),
            Some(concat!(
                "mojoaar\nhttps://johansen.foo\n",
                env!("CARGO_PKG_LICENSE")
            ))
        );
        assert_eq!(meta.copyright.as_deref(), Some("Copyright © 2026 mojoaar"));
    }

    #[test]
    fn window_config() {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(v["productName"], "Sigrun");
        assert_eq!(v["identifier"], "com.sigrun.app");
        assert_eq!(v["app"]["windows"][0]["title"], "Sigrun");
        assert_eq!(v["app"]["windows"][0]["minWidth"], 900);
    }
}
