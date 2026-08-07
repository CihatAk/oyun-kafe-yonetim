use rusqlite::params;
use tauri::State;
use uuid::Uuid;

use crate::db::AppState;
use crate::models::*;
use crate::commands::auth::log_audit_conn;

#[tauri::command]
pub fn get_stations(state: State<AppState>) -> Result<Vec<Station>, String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let mut stmt = conn.prepare(
        "SELECT s.id, s.name, s.station_type, s.status, COALESCE(s.group_name, ''),
                COUNT(h.id) as total_sessions, COALESCE(SUM(h.total), 0) as total_revenue
         FROM stations s LEFT JOIN session_history h ON s.name = h.station_name
         GROUP BY s.id ORDER BY s.name"
    ).map_err(|e| e.to_string())?;
    let stations = stmt.query_map([], |row| {
        Ok(Station {
            id: row.get(0)?, name: row.get(1)?, station_type: row.get(2)?,
            status: row.get(3)?, group_name: row.get(4)?,
            total_sessions: row.get(5)?, total_revenue: row.get(6)?,
        })
    }).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).collect();
    Ok(stations)
}

#[tauri::command]
pub fn add_station(name: String, group_name: String, station_type: Option<String>, state: State<AppState>) -> Result<Station, String> {
    crate::commands::auth::require_admin(&state)?;
    let st_type = match station_type.as_deref().unwrap_or("standard") { "vr" => "vr", _ => "standard" };
    let id = format!("pc-{}", &Uuid::new_v4().to_string()[..8]);
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    conn.execute("INSERT INTO stations (id, name, station_type, status, group_name) VALUES (?1, ?2, ?3, 'idle', ?4)", params![id, name, st_type, group_name]).map_err(|e| format!("İstasyon eklenemedi: {}", e))?;
    log_audit_conn(&conn, &state, "add_station", "stations", &name);
    Ok(Station { id, name, station_type: st_type.into(), status: "idle".into(), group_name, total_sessions: 0, total_revenue: 0.0 })
}

#[tauri::command]
pub fn update_station(station_id: String, name: String, group_name: String, station_type: Option<String>, state: State<AppState>) -> Result<(), String> {
    crate::commands::auth::require_admin(&state)?;
    let st_type = match station_type.as_deref().unwrap_or("standard") { "vr" => "vr", _ => "standard" };
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let old_name: Option<String> = conn.query_row("SELECT name FROM stations WHERE id = ?1", params![station_id], |r| r.get(0)).ok();
    conn.execute_batch("BEGIN TRANSACTION").map_err(|e| e.to_string())?;
    let tx_result: Result<(), String> = (|| {
        conn.execute("UPDATE stations SET name = ?1, group_name = ?2, station_type = ?3 WHERE id = ?4", params![name, group_name, st_type, station_id]).map_err(|e| e.to_string())?;
        if let Some(old) = old_name {
            if old != name {
                conn.execute("UPDATE active_sessions SET station_name = ?1 WHERE station_id = ?2", params![name, station_id]).map_err(|e| e.to_string())?;
                conn.execute("UPDATE session_history SET station_name = ?1 WHERE station_name = ?2", params![name, old]).map_err(|e| e.to_string())?;
                conn.execute("UPDATE drink_orders SET station_name = ?1 WHERE station_name = ?2", params![name, old]).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    })();
    if let Err(e) = tx_result {
        conn.execute_batch("ROLLBACK").ok();
        return Err(e);
    }
    conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "update_station", "stations", &name);
    Ok(())
}

#[tauri::command]
pub fn remove_station(station_id: String, state: State<AppState>) -> Result<(), String> {
    crate::commands::auth::require_admin(&state)?;
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let active: bool = conn.query_row("SELECT COUNT(*) > 0 FROM active_sessions WHERE station_id = ?1", params![station_id], |r| r.get(0)).unwrap_or(false);
    if active { return Err("Aktif oturumu olan istasyon silinemez".into()); }
    conn.execute("DELETE FROM stations WHERE id = ?1", params![station_id]).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "remove_station", "stations", &station_id);
    Ok(())
}
