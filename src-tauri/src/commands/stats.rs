use chrono::Local;
use rusqlite::params;
use tauri::State;

use crate::db::AppState;
use crate::queries::session_fee;

#[tauri::command]
pub fn get_dashboard_stats(state: State<AppState>) -> Result<serde_json::Value, String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let today = Local::now().date_naive().to_string();
    let active: i64 = conn.query_row("SELECT COUNT(*) FROM active_sessions", [], |r| r.get(0)).unwrap_or(0);
    let idle: i64 = conn.query_row("SELECT COUNT(*) FROM stations WHERE status='idle'", [], |r| r.get(0)).unwrap_or(0);
    let total_st: i64 = conn.query_row("SELECT COUNT(*) FROM stations", [], |r| r.get(0)).unwrap_or(0);
    let today_rev: f64 = conn.query_row("SELECT COALESCE(SUM(total),0) FROM session_history WHERE date(end_time)=?1", params![today], |r| r.get(0)).unwrap_or(0.0);
    let total_sess: i64 = conn.query_row("SELECT COUNT(*) FROM session_history", [], |r| r.get(0)).unwrap_or(0);
    let avg_dur: i64 = conn.query_row("SELECT COALESCE(AVG(duration_minutes),0) FROM session_history", [], |r| r.get(0)).unwrap_or(0);
    let last_week: f64 = conn.query_row("SELECT COALESCE(SUM(total),0) FROM session_history WHERE date(end_time)>=date(?1,'-7 days') AND date(end_time)<?1", params![today], |r| r.get(0)).unwrap_or(0.0);

    // Optimize: tek sorgu ile saatlik gelir (24 ayrı sorgu yerine)
    let mut hourly = vec![0.0f64; 24];
    if let Ok(mut stmt) = conn.prepare("SELECT cast(strftime('%H',end_time) as integer) as h, COALESCE(SUM(total),0) FROM session_history WHERE date(end_time)=?1 GROUP BY h") {
        if let Ok(rows) = stmt.query_map(params![today], |row| Ok((row.get::<_, usize>(0)?, row.get::<_, f64>(1)?))) {
            for r in rows.flatten() {
                if r.0 < 24 { hourly[r.0] = r.1; }
            }
        }
    }
    let hourly_json: Vec<serde_json::Value> = (0..24).map(|h| serde_json::json!({"hour": h, "revenue": hourly[h]})).collect();

    Ok(serde_json::json!({ "active_count": active, "idle_count": idle, "total_stations": total_st, "today_revenue": today_rev, "total_sessions": total_sess, "avg_duration": avg_dur, "hourly_revenue": hourly_json, "last_week_revenue": last_week }))
}

#[tauri::command]
pub fn calculate_live_fee(station_id: String, payment_method: String, state: State<AppState>) -> Result<serde_json::Value, String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let (start_str, paused_at, total_paused, extra_controllers, rate_type, st_type): (String, Option<String>, i64, i64, String, String) = conn.query_row(
        "SELECT a.start_time, a.paused_at, COALESCE(a.total_paused_seconds,0), COALESCE(a.extra_controllers,0), COALESCE(a.rate_type,'nakit'), COALESCE(s.station_type,'standard') FROM active_sessions a LEFT JOIN stations s ON a.station_id=s.id WHERE a.station_id=?1",
        params![station_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
    ).map_err(|_| "Oturum bulunamadı")?;
    let now = Local::now();
    let start: chrono::DateTime<Local> = chrono::DateTime::parse_from_rfc3339(&start_str).map_err(|e| e.to_string())?.with_timezone(&Local);
    let rate = crate::queries::rate_type_for(&payment_method, &rate_type);
    let total_secs = now.signed_duration_since(start).num_seconds().max(0);
    let (dur_mins, dur_secs, fee, extra_fee, is_paused) =
        session_fee(&conn, &station_id, rate, &start_str, paused_at.as_deref(), total_paused, extra_controllers);
    let pricing = AppState::load_pricing_conn(&conn);
    let per_min = if rate == "nakit" { pricing.cash_per_minute } else { pricing.card_per_minute };
    let paused_seconds = if let Some(ref p) = paused_at {
        if let Ok(pd) = chrono::DateTime::parse_from_rfc3339(p) {
            now.signed_duration_since(pd.with_timezone(&Local)).num_seconds().max(0)
        } else { 0 }
    } else { 0 };
    let dt: f64 = conn.query_row("SELECT COALESCE(SUM(total),0) FROM drink_orders WHERE session_id=?1", params![station_id], |r| r.get(0)).unwrap_or(0.0);
    let max_secs = pricing.max_session_minutes * 60;
    let warn_secs = pricing.warning_before_minutes * 60;
    Ok(serde_json::json!({
        "minutes": dur_mins, "seconds": dur_secs, "current_fee": fee, "per_minute": per_min,
        "extra_controllers": extra_controllers, "extra_fee": extra_fee, "extra_controller_per_hour": pricing.extra_controller_per_hour,
        "drink_total": dt, "total_with_drinks": fee + extra_fee + dt, "is_paused": is_paused,
        "paused_seconds": paused_seconds, "station_type": st_type,
        "show_warning": pricing.max_session_minutes > 0 && total_secs >= (max_secs - warn_secs) && total_secs < max_secs,
        "auto_end": pricing.max_session_minutes > 0 && total_secs >= max_secs,
        "remaining_seconds": if max_secs > 0 { (max_secs - total_secs).max(0) } else { 0 },
    }))
}
