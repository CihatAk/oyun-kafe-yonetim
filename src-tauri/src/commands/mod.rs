pub mod stations;
pub mod sessions;
pub mod drinks;
pub mod finance;
pub mod stats;
pub mod auth;
pub mod settings;
pub mod campaigns;

use tauri::AppHandle;
use tauri::Manager;

#[tauri::command]
pub fn toggle_fullscreen(app: AppHandle) {
    if let Some(w) = app.get_window("main") {
        let is_full = w.is_fullscreen().unwrap_or(false);
        w.set_fullscreen(!is_full).ok();
    }
}
