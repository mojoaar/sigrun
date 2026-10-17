mod host;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .setup(|app| {
            let paths = sigrun_core::paths::Paths::resolve()?;
            let state = tauri::async_runtime::block_on(host::start_host(paths))?;
            app.manage(state);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Sigrun")
        .run(|app_handle, event| {
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
