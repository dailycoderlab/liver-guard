//! LiverGuard desktop: a thin Tauri shell over the shared Rust core.

mod commands;
mod platform;

use commands::*;

pub fn run() {
    platform::prepare_process();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            list_scripts,
            get_script,
            create_script,
            save_script,
            rename_script,
            duplicate_script,
            export_script,
            import_script,
            get_notify_config,
            save_notify_config,
            send_test_notification,
            delete_script,
            validate_ir,
            list_windows,
            permissions,
            request_permissions,
            open_permission_settings,
            live_start,
            live_stop,
            snapshot,
            load_image,
            detect_candidates,
            create_template,
            asset_png,
            run_start,
            run_stop,
            probe,
            list_runs,
            get_run,
        ])
        .run(tauri::generate_context!())
        .expect("error while running LiverGuard");
}
