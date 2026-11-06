//! LiverGuard desktop: a thin Tauri shell over the shared Rust core.

mod commands;
mod platform;

use commands::*;

pub fn run() {
    platform::prepare_process();
    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            list_scripts,
            get_script,
            create_script,
            save_script,
            rename_script,
            delete_script,
            validate_ir,
            list_windows,
            permissions,
            request_permissions,
            open_permission_settings,
            live_start,
            live_stop,
            snapshot,
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
