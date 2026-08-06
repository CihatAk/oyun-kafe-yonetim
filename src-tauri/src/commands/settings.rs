use rusqlite::params;
use tauri::State;

use crate::db::AppState;
use crate::commands::auth::log_audit_conn;

pub const DEFAULT_BUSINESS_NAME: &str = "JiJi Game Center - PlayStation & VR";

pub fn get_setting_value(conn: &rusqlite::Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| r.get(0)).ok()
}

pub fn get_business_name(conn: &rusqlite::Connection) -> String {
    let name = get_setting_value(conn, "business_name")
        .unwrap_or_else(|| DEFAULT_BUSINESS_NAME.to_string());
    if name.trim().is_empty() {
        DEFAULT_BUSINESS_NAME.to_string()
    } else {
        name.trim().to_string()
    }
}

#[derive(Clone, serde::Serialize)]
pub struct UiConfig {
    pub business_name: String,
    pub business_name_upper: String,
}

#[tauri::command]
pub fn get_ui_config(state: State<AppState>) -> Result<UiConfig, String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let name = get_business_name(&conn);
    Ok(UiConfig {
        business_name_upper: name.to_uppercase(),
        business_name: name,
    })
}

#[tauri::command]
pub fn set_business_name(name: String, state: State<AppState>) -> Result<(), String> {
    crate::commands::auth::require_admin(&state)?;
    let name = name.trim().to_string();
    if name.is_empty() { return Err("İşletme adı boş olamaz!".into()); }
    if name.len() > 80 { return Err("İşletme adı en fazla 80 karakter olabilir.".into()); }
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    set_setting_value(&conn, "business_name", &name)?;
    log_audit_conn(&conn, &state, "set_setting", "settings", format!("İşletme adı güncellendi: {}", name).as_str());
    Ok(())
}

pub fn set_setting_value(conn: &rusqlite::Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value]).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_low_stock_threshold(state: State<AppState>) -> Result<i64, String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    Ok(get_setting_value(&conn, "low_stock_threshold").and_then(|v| v.parse().ok()).unwrap_or(5))
}

#[tauri::command]
pub fn set_low_stock_threshold(value: i64, state: State<AppState>) -> Result<(), String> {
    crate::commands::auth::require_admin(&state)?;
    let threshold = value.max(0);
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    set_setting_value(&conn, "low_stock_threshold", &threshold.to_string())?;
    log_audit_conn(&conn, &state, "set_setting", "settings", format!("Düşük stok eşiği: {}", threshold).as_str());
    Ok(())
}
