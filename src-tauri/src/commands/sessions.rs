use chrono::{DateTime, Local};
use rusqlite::params;
use tauri::State;
use uuid::Uuid;

use crate::db::AppState;
use crate::models::*;
use crate::commands::auth::{log_audit_conn, require_login};

fn parse_partials(pp: Option<&str>) -> Result<Vec<(String, f64)>, String> {
    match pp {
        None => Ok(Vec::new()),
        Some(s) => {
            let vals: Vec<serde_json::Value> = serde_json::from_str(s).map_err(|e| format!("Kısmi ödeme verisi geçersiz: {}", e))?;
            vals.into_iter().map(|v| {
                let method = v.get("method").and_then(|m| m.as_str()).ok_or_else(|| "Kısmi ödeme 'method' alanı eksik".to_string())?.to_string();
                let amount = v.get("amount").and_then(|a| a.as_f64()).ok_or_else(|| "Kısmi ödeme 'amount' alanı geçersiz".to_string())?;
                if amount <= 0.0 { return Err("Kısmi ödeme tutarı 0'dan büyük olmalı!".into()); }
                Ok((method, amount))
            }).collect()
        }
    }
}

fn map_active(row: &rusqlite::Row) -> rusqlite::Result<ActiveSession> {
    Ok(ActiveSession {
        station_id: row.get(0)?, station_name: row.get(1)?, customer: row.get(2)?,
        start_time: row.get(3)?, rate_type: row.get(4)?, notes: row.get(5)?,
        tags: row.get(6)?, paused_at: row.get(7)?, total_paused_seconds: row.get(8)?,
        extra_controllers: row.get(9)?,
    })
}

#[tauri::command]
pub fn start_session(station_id: String, customer: String, rate_type: String, notes: String, tags: String, extra_controllers: Option<i64>, state: State<AppState>) -> Result<ActiveSession, String> {
    require_login(&state)?;
    let extra = extra_controllers.unwrap_or(0).max(0);
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let (status, st_name): (String, String) = conn.query_row("SELECT status, name FROM stations WHERE id = ?1", params![station_id], |row| Ok((row.get(0)?, row.get(1)?))).map_err(|_| "İstasyon bulunamadı")?;
    if status == "active" { return Err("İstasyon zaten aktif".into()); }
    conn.execute("UPDATE stations SET status = 'active' WHERE id = ?1", params![station_id]).map_err(|e| e.to_string())?;
    let session = ActiveSession {
        station_id: station_id.clone(), station_name: st_name, customer,
        start_time: Local::now().to_rfc3339(), rate_type, notes, tags,
        paused_at: None, total_paused_seconds: 0, extra_controllers: extra,
    };
    conn.execute("INSERT INTO active_sessions (station_id, station_name, customer, start_time, rate_type, notes, tags, paused_at, total_paused_seconds, extra_controllers) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, 0, ?8)",
        params![session.station_id, session.station_name, session.customer, session.start_time, session.rate_type, session.notes, session.tags, extra]).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "start_session", "sessions", format!("{} (ekstra kol: {})", session.station_name, extra).as_str());
    Ok(session)
}

#[tauri::command]
pub fn pause_session(station_id: String, state: State<AppState>) -> Result<(), String> {
    require_login(&state)?;
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let p: Option<String> = conn.query_row("SELECT paused_at FROM active_sessions WHERE station_id = ?1", params![station_id], |r| r.get(0)).map_err(|_| "Oturum bulunamadı")?;
    if p.is_some() { return Err("Zaten duraklatılmış".into()); }
    conn.execute("UPDATE active_sessions SET paused_at = ?1 WHERE station_id = ?2", params![Local::now().to_rfc3339(), station_id]).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "pause_session", "sessions", &station_id);
    Ok(())
}

#[tauri::command]
pub fn resume_session(station_id: String, state: State<AppState>) -> Result<(), String> {
    require_login(&state)?;
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let p: Option<String> = conn.query_row("SELECT paused_at FROM active_sessions WHERE station_id = ?1", params![station_id], |r| r.get(0)).map_err(|_| "Oturum bulunamadı")?;
    match p {
        None => Err("Oturum zaten aktif".into()),
        Some(paused) => {
            let pause_dt: DateTime<Local> = DateTime::parse_from_rfc3339(&paused).map_err(|e| e.to_string())?.with_timezone(&Local);
            let secs = Local::now().signed_duration_since(pause_dt).num_seconds().max(0);
            let cur: i64 = conn.query_row("SELECT total_paused_seconds FROM active_sessions WHERE station_id = ?1", params![station_id], |r| r.get(0)).unwrap_or(0);
            conn.execute("UPDATE active_sessions SET paused_at = NULL, total_paused_seconds = ?1 WHERE station_id = ?2", params![cur + secs, station_id]).map_err(|e| e.to_string())?;
            log_audit_conn(&conn, &state, "resume_session", "sessions", &station_id);
            Ok(())
        }
    }
}

#[tauri::command]
pub fn update_session_notes(station_id: String, notes: String, tags: String, state: State<AppState>) -> Result<(), String> {
    require_login(&state)?;
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    conn.execute("UPDATE active_sessions SET notes = ?1, tags = ?2 WHERE station_id = ?3", params![notes, tags, station_id]).map_err(|_| "Oturum bulunamadı")?;
    log_audit_conn(&conn, &state, "update_session_notes", "sessions", &station_id);
    Ok(())
}

#[tauri::command]
pub fn get_active_sessions(state: State<AppState>) -> Result<Vec<ActiveSession>, String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let mut stmt = conn.prepare("SELECT station_id, station_name, customer, start_time, rate_type, COALESCE(notes,''), COALESCE(tags,''), paused_at, COALESCE(total_paused_seconds,0), COALESCE(extra_controllers,0) FROM active_sessions").map_err(|e| e.to_string())?;
    let sessions: Vec<ActiveSession> = stmt.query_map([], map_active).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).collect();
    Ok(sessions)
}

#[tauri::command]
pub fn update_session_start_time(station_id: String, new_start_time: String, state: State<AppState>) -> Result<ActiveSession, String> {
    require_login(&state)?;
    let _p: DateTime<Local> = DateTime::parse_from_rfc3339(&new_start_time).map_err(|e| format!("Geçersiz tarih: {}", e))?.with_timezone(&Local);
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    conn.execute("UPDATE active_sessions SET start_time = ?1 WHERE station_id = ?2", params![new_start_time, station_id]).map_err(|_| "Oturum bulunamadı")?;
    log_audit_conn(&conn, &state, "update_session_start_time", "sessions", &station_id);
    conn.query_row("SELECT station_id, station_name, customer, start_time, rate_type, COALESCE(notes,''), COALESCE(tags,''), paused_at, COALESCE(total_paused_seconds,0), COALESCE(extra_controllers,0) FROM active_sessions WHERE station_id = ?1",
        params![station_id], map_active).map_err(|_| "Oturum bulunamadı".into())
}

#[tauri::command]
pub fn update_session_extra_controllers(station_id: String, extra_controllers: i64, state: State<AppState>) -> Result<(), String> {
    require_login(&state)?;
    let extra = extra_controllers.max(0);
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    conn.execute("UPDATE active_sessions SET extra_controllers = ?1 WHERE station_id = ?2", params![extra, station_id]).map_err(|_| "Oturum bulunamadı")?;
    log_audit_conn(&conn, &state, "update_session_extra_controllers", "sessions", format!("{} ekstra kol: {}", station_id, extra).as_str());
    Ok(())
}

#[tauri::command]
pub fn end_session(station_id: String, payment_method: String, custom_end_time: Option<String>, partial_payments_json: Option<String>, discount_amount: Option<f64>, discount_reason: Option<String>, state: State<AppState>) -> Result<SessionRecord, String> {
    require_login(&state)?;
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;

    let row = conn.query_row(
        "SELECT station_name, customer, start_time, rate_type, COALESCE(notes,''), COALESCE(tags,''), paused_at, COALESCE(total_paused_seconds,0), COALESCE(extra_controllers,0) FROM active_sessions WHERE station_id = ?1",
        params![station_id],
        |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?, r.get::<_,String>(2)?, r.get::<_,String>(3)?, r.get::<_,String>(4)?, r.get::<_,String>(5)?, r.get::<_,Option<String>>(6)?, r.get::<_,i64>(7)?, r.get::<_,i64>(8)?)),
    ).map_err(|_| "Aktif oturum bulunamadı")?;
    let (station_name, customer, start_str, rate_type, notes, tags, paused_at, total_paused, extra_controllers) = row;

    let st_type: String = conn.query_row("SELECT station_type FROM stations WHERE name = ?1", params![station_name], |r| r.get(0)).unwrap_or_else(|_| "standard".into());
    let start: DateTime<Local> = DateTime::parse_from_rfc3339(&start_str).map_err(|e| e.to_string())?.with_timezone(&Local);
    let end: DateTime<Local> = if let Some(ref c) = custom_end_time {
        DateTime::parse_from_rfc3339(c).map_err(|e| format!("Geçersiz tarih: {}", e))?.with_timezone(&Local)
    } else { Local::now() };

    let total_dur = end.signed_duration_since(start);
    let mut eff_secs = total_dur.num_seconds().max(0);
    if let Some(ref p) = paused_at {
        if let Ok(pd) = DateTime::parse_from_rfc3339(p) {
            let ps = end.signed_duration_since(pd.with_timezone(&Local)).num_seconds().max(0);
            eff_secs = eff_secs.saturating_sub(ps);
        }
    }
    eff_secs = eff_secs.saturating_sub(total_paused);
    let dur_mins = (eff_secs / 60).max(0);

    let pricing = AppState::load_pricing_conn(&conn);
    let per_min = state.get_effective_rate(&st_type, &rate_type, &pricing);
    let round_mins = pricing.round_minutes.max(1);
    let chunks = ((dur_mins as f64) / (round_mins as f64)).ceil() as i64;
    let rounded_mins = chunks * round_mins;

    let drink_total: f64 = conn.query_row("SELECT COALESCE(SUM(total), 0) FROM drink_orders WHERE session_id = ?1", params![station_id], |r| r.get(0)).unwrap_or(0.0);
    let base_fee = (rounded_mins as f64 * per_min).max(pricing.min_charge);
    let extra_per_min = pricing.extra_controller_per_hour / 60.0;
    let extra_fee = extra_controllers.max(0) as f64 * extra_per_min * (rounded_mins as f64);

    let mut total = base_fee + extra_fee + drink_total;

    let current_user = state.current_user.lock().map_err(|e| e.to_string())?.clone();
    let is_admin = matches!(&current_user, Some(u) if u.role == "admin");
    let discount_limit = current_user.as_ref().map(|u| u.discount_limit()).unwrap_or(0.0);

    let mut discount = 0.0;
    let mut fee_note = String::new();
    let mut discount_reason_saved = String::new();
    let requested = discount_amount.unwrap_or(0.0).max(0.0);
    if requested > 0.0 {
        if !is_admin && discount_limit <= 0.0 {
            return Err("Bu kullanıcı için manuel indirim yetkisi tanımlı değil!".into());
        }
        if !is_admin && requested > discount_limit {
            return Err(format!("İndirim limiti aşıldı! Bu kullanıcının limiti ₺{:.2}", discount_limit));
        }
        let applied = requested.min(total);
        discount = applied;
        total = (total - applied).max(0.0);
        let reason = discount_reason.unwrap_or_default().trim().to_string();
        discount_reason_saved = reason.clone();
        fee_note = if reason.is_empty() {
            format!("Manuel indirim: ₺{:.2}", applied)
        } else {
            format!("Manuel indirim: ₺{:.2} ({})", applied, reason)
        };
    }

    let partials = parse_partials(partial_payments_json.as_deref())?;
    let (total_final, final_pm) = if partial_payments_json.is_some() {
        let desc = partials.iter().map(|(m, a)| format!("{}:{:.2}", m, a)).collect::<Vec<_>>().join(",");
        (total, format!("{}+kısmi:{}", payment_method, desc))
    } else {
        (total, payment_method.clone())
    };

    // Transaction ile tutarlılık sağla
    conn.execute_batch("BEGIN TRANSACTION").map_err(|e| e.to_string())?;
    let mut hist_id = String::new();
    let tx_result: Result<(), String> = (|| {
        conn.execute("DELETE FROM active_sessions WHERE station_id = ?1", params![station_id]).map_err(|e| e.to_string())?;
        conn.execute("UPDATE stations SET status = 'idle' WHERE id = ?1", params![station_id]).map_err(|e| e.to_string())?;

        for (m, a) in &partials {
            conn.execute("INSERT INTO partial_payments (id, session_id, payment_method, amount, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![Uuid::new_v4().to_string(), station_id, m, a, Local::now().to_rfc3339()]).map_err(|e| e.to_string())?;
        }

        let hist_notes = if !fee_note.is_empty() { fee_note.clone() } else { notes.clone() };
        let hist_id_inner = Uuid::new_v4().to_string();
        hist_id = hist_id_inner.clone();
        conn.execute("INSERT INTO session_history (id, station_name, customer, start_time, end_time, duration_minutes, total, payment_method, rate_type, drink_total, discount, discount_reason, notes, tags, extra_controllers, extra_fee) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
            params![hist_id_inner, station_name, customer, start.to_rfc3339(), end.to_rfc3339(), dur_mins, total_final, final_pm, rate_type, drink_total, discount, discount_reason_saved, hist_notes, tags, extra_controllers, extra_fee]).map_err(|e| e.to_string())?;
        conn.execute("UPDATE drink_orders SET session_id = ?1 WHERE session_id = ?2", params![hist_id_inner, station_id]).map_err(|e| e.to_string())?;
        conn.execute("UPDATE partial_payments SET session_id = ?1 WHERE session_id = ?2", params![hist_id_inner, station_id]).map_err(|e| e.to_string())?;
        let audit_detail = if discount > 0.0 {
            format!("{} (₺{:.2}, indirim: ₺{:.2})", station_name, total_final, discount)
        } else {
            format!("{} (₺{:.2})", station_name, total_final)
        };
        log_audit_conn(&conn, &state, "end_session", "sessions", audit_detail.as_str());
        Ok(())
    })();

    if let Err(e) = tx_result {
        conn.execute_batch("ROLLBACK").ok();
        return Err(e);
    }
    conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;

    Ok(SessionRecord { id: hist_id, station_name, customer, start_time: start.to_rfc3339(), end_time: end.to_rfc3339(), duration_minutes: dur_mins, total: total_final, payment_method: final_pm, rate_type, drink_total, discount, notes, tags, extra_controllers, extra_fee })
}
