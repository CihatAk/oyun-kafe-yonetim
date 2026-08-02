use chrono::{Datelike, Local};
use rusqlite::params;
use tauri::State;

use crate::db::AppState;

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
    let (start_str, paused_at, total_paused, st_type): (String, Option<String>, i64, String) = conn.query_row(
        "SELECT a.start_time, a.paused_at, COALESCE(a.total_paused_seconds,0), COALESCE(s.station_type,'standard') FROM active_sessions a LEFT JOIN stations s ON a.station_id=s.id WHERE a.station_id=?1",
        params![station_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).map_err(|_| "Oturum bulunamadı")?;
    let start: chrono::DateTime<Local> = chrono::DateTime::parse_from_rfc3339(&start_str).map_err(|e| e.to_string())?.with_timezone(&Local);
    let now = Local::now();
    let total_secs = now.signed_duration_since(start).num_seconds().max(0);
    let is_paused = paused_at.is_some();
    let mut eff_secs = total_secs;
    let mut paused_remaining = 0i64;
    if let Some(ref p) = paused_at {
        if let Ok(pd) = chrono::DateTime::parse_from_rfc3339(p) {
            paused_remaining = now.signed_duration_since(pd.with_timezone(&Local)).num_seconds().max(0);
            eff_secs = eff_secs.saturating_sub(paused_remaining);
        }
    }
    eff_secs = eff_secs.saturating_sub(total_paused);
    let dur_mins = (eff_secs / 60).max(0);
    let dur_secs = eff_secs.max(0) % 60;
    let pricing = AppState::load_pricing_conn(&conn);
    let per_min = state.get_effective_rate(&st_type, &payment_method, &pricing);
    let round_mins = pricing.round_minutes.max(1);
    let chunks = ((dur_mins as f64) / (round_mins as f64)).ceil() as i64;
    let rounded = chunks * round_mins;
    let fee = (rounded as f64 * per_min).max(pricing.min_charge);
    let dt: f64 = conn.query_row("SELECT COALESCE(SUM(total),0) FROM drink_orders WHERE session_id=?1", params![station_id], |r| r.get(0)).unwrap_or(0.0);
    let max_secs = pricing.max_session_minutes * 60;
    let warn_secs = pricing.warning_before_minutes * 60;
    Ok(serde_json::json!({
        "minutes": dur_mins, "seconds": dur_secs, "current_fee": fee, "per_minute": per_min,
        "drink_total": dt, "total_with_drinks": fee + dt, "is_paused": is_paused,
        "paused_seconds": paused_remaining, "station_type": st_type,
        "show_warning": pricing.max_session_minutes > 0 && total_secs >= (max_secs - warn_secs) && total_secs < max_secs,
        "auto_end": pricing.max_session_minutes > 0 && total_secs >= max_secs,
        "remaining_seconds": if max_secs > 0 { (max_secs - total_secs).max(0) } else { 0 },
    }))
}

#[tauri::command]
pub fn get_revenue_by_period(state: State<AppState>, period: String) -> Result<serde_json::Value, String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let today = Local::now().date_naive();
    let (labels, count): (Vec<String>, usize) = match period.as_str() {
        "daily" => ((0..7).rev().map(|i| (today - chrono::Duration::days(i)).format("%d/%m").to_string()).collect(), 7),
        "weekly" => ((0..4).rev().map(|i| { let d = today - chrono::Duration::weeks(i); format!("{}-{}", d.format("%d/%m"), (d+chrono::Duration::days(6)).format("%d/%m")) }).collect(), 4),
        "monthly" => ((0..6).rev().map(|i| { let d = today - chrono::Duration::days(i*30); d.format("%B %Y").to_string() }).collect(), 6),
        _ => ((0..7).rev().map(|i| (today - chrono::Duration::days(i)).format("%d/%m").to_string()).collect(), 7),
    };
    let values: Vec<f64> = (0..count).map(|i| {
        match period.as_str() {
            "daily" => { let d = (today - chrono::Duration::days((count-1-i) as i64)).to_string(); conn.query_row("SELECT COALESCE(SUM(total),0) FROM session_history WHERE date(end_time)=?1", params![d], |r| r.get(0)).unwrap_or(0.0) }
            "weekly" => { let ws = today - chrono::Duration::weeks((count-1-i) as i64); let we = ws + chrono::Duration::days(6); conn.query_row("SELECT COALESCE(SUM(total),0) FROM session_history WHERE date(end_time)>=?1 AND date(end_time)<=?2", params![ws.to_string(), we.to_string()], |r| r.get(0)).unwrap_or(0.0) }
            "monthly" => {
                let rd = today - chrono::Duration::days(((count-1-i) as i64)*30);
                let ms = rd.with_day(1).unwrap_or(rd);
                let nm = if rd.month()==12 {
                    if let Some(y) = rd.with_year(rd.year()+1) { y.with_month(1).unwrap_or(y) } else { rd }
                } else { rd.with_month(rd.month()+1).unwrap_or(rd) };
                let me = nm - chrono::Duration::days(1);
                conn.query_row("SELECT COALESCE(SUM(total),0) FROM session_history WHERE date(end_time)>=?1 AND date(end_time)<=?2", params![ms.to_string(), me.to_string()], |r| r.get(0)).unwrap_or(0.0)
            }
            _ => 0.0,
        }
    }).collect();
    Ok(serde_json::json!({"labels": labels, "values": values}))
}

#[tauri::command]
pub fn get_top_stations(state: State<AppState>) -> Result<serde_json::Value, String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let mut stmt = conn.prepare("SELECT station_name, COUNT(*) as cnt, SUM(total) as rev FROM session_history GROUP BY station_name ORDER BY cnt DESC LIMIT 10").map_err(|e| e.to_string())?;
    let rows: Vec<(String,i64,f64)> = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).collect();
    Ok(serde_json::json!({ "names": rows.iter().map(|r| &r.0).collect::<Vec<_>>(), "counts": rows.iter().map(|r| r.1).collect::<Vec<_>>(), "revenues": rows.iter().map(|r| r.2).collect::<Vec<_>>() }))
}

#[tauri::command]
pub fn get_top_drinks(state: State<AppState>) -> Result<serde_json::Value, String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let mut stmt = conn.prepare("SELECT drink_name, SUM(quantity) as qty, SUM(total) as rev FROM drink_orders GROUP BY drink_name ORDER BY qty DESC LIMIT 10").map_err(|e| e.to_string())?;
    let rows: Vec<(String,i64,f64)> = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).collect();
    Ok(serde_json::json!({ "names": rows.iter().map(|r| &r.0).collect::<Vec<_>>(), "quantities": rows.iter().map(|r| r.1).collect::<Vec<_>>(), "revenues": rows.iter().map(|r| r.2).collect::<Vec<_>>() }))
}

#[tauri::command]
pub fn get_duration_trend(state: State<AppState>) -> Result<serde_json::Value, String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let today = Local::now().date_naive();

    // Optimize: tek sorgu ile  günlük trend (30 ayrı sorgu yerine)
    let mut day_map = std::collections::HashMap::new();
    if let Ok(mut stmt) = conn.prepare("SELECT date(end_time) as d, COALESCE(AVG(duration_minutes),0) FROM session_history WHERE date(end_time) >= date(?1, '-30 days') AND date(end_time) <= ?1 GROUP BY d") {
        if let Ok(rows) = stmt.query_map(params![today.to_string()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))) {
            for r in rows.flatten() {
                day_map.insert(r.0, (r.1 * 10.0).round() / 10.0);
            }
        }
    }

    let mut labels = Vec::with_capacity(30);
    let mut values = Vec::with_capacity(30);
    for i in (0..30).rev() {
        let d = today - chrono::Duration::days(i);
        labels.push(d.format("%d/%m").to_string());
        values.push(day_map.get(&d.to_string()).copied().unwrap_or(0.0));
    }

    Ok(serde_json::json!({"labels": labels, "values": values}))
}
