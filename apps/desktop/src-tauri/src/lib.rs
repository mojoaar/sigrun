mod commands;
mod host;
mod tray;

use tauri::Manager;

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
                    loop {
                        if st.shutdown.is_cancelled() {
                            return;
                        }
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
    fn window_config() {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(v["productName"], "Sigrun");
        assert_eq!(v["identifier"], "com.sigrun.app");
        assert_eq!(v["app"]["windows"][0]["title"], "Sigrun");
        assert_eq!(v["app"]["windows"][0]["minWidth"], 900);
    }
}
