#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod models;
mod db;
mod queries;
mod sync;
mod web;
mod license;
mod commands;

use db::AppState;
use commands::*;
use tauri::Manager;

fn main() {
    let app_state = AppState::new();
    tauri::Builder::default()
        .manage(app_state)
        // Pencere gizli başlar; sayfa yüklendiğinde (stillerle birlikte) gösterilir.
        // Böylece WebView2'nin ilk çizim gecikmesi beyaz ekran olarak görünmez.
        .on_page_load(|window, _payload| {
            let _ = window.show();
        })
        .invoke_handler(tauri::generate_handler![
            stations::get_stations, stations::add_station, stations::update_station, stations::remove_station,
            sessions::start_session, sessions::end_session, sessions::get_active_sessions,
            sessions::update_session_start_time, sessions::pause_session, sessions::resume_session,
            sessions::update_session_notes, sessions::update_session_extra_controllers,
            sessions::transfer_session,
            finance::get_pricing, finance::set_pricing, finance::get_history,
            finance::get_history_filtered, finance::clear_history, finance::delete_history,
            finance::export_history_csv, finance::export_history_json,
            finance::backup_database, finance::get_receipt, finance::get_day_end_report,
            stats::get_dashboard_stats, stats::calculate_live_fee,
            drinks::get_drinks, drinks::add_drink, drinks::update_drink,
            drinks::set_drink_active, drinks::remove_drink, drinks::order_drink,
            drinks::get_drink_orders, drinks::remove_drink_order,
            drinks::get_session_drink_items, drinks::adjust_stock,
            drinks::get_stock_movements, drinks::get_low_stock_items, drinks::get_stock_report,
            settings::get_low_stock_threshold, settings::set_low_stock_threshold,
            settings::get_ui_config, settings::set_business_name,
            license::get_license_status, license::activate_license,
            auth::login, auth::logout, auth::get_current_user,
            auth::list_users, auth::add_user, auth::update_user, auth::remove_user,
            auth::change_password, auth::force_change_password, auth::reset_user_password,
            auth::get_audit_log,
            web::get_web_info, web::get_sync_status, web::get_supabase_config_info, web::save_supabase_config,
            toggle_fullscreen,
        ])
        .setup(|app| {
            let db_path = db::get_db_path();
            let port = std::env::var("OYUNKAFE_WEB_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(web::WEB_PORT);
            std::thread::spawn(move || web::run(db_path, port));
            sync::start();
            license::start_license_watch();
            // Güvenlik ağı: on_page_load tetiklenmezse pencereyi yine de göster
            let handle = app.handle();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(6));
                if let Some(w) = handle.get_window("main") {
                    let _ = w.show();
                }
            });
            // Otomatik güncelleme kontrolü için event (frontend'de handle edilir)
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(10));
                let _ = app_handle.emit_all("check-update", ());
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Uygulama başlatılırken hata oluştu");
}
