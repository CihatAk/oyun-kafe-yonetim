use chrono::{DateTime, Local, Timelike, Datelike, Weekday};
use rusqlite::{params, Connection};
use tauri::State;
use uuid::Uuid;

use crate::db::AppState;
use crate::models::*;
use crate::commands::auth::log_audit_conn;

fn now() -> String {
    Local::now().to_rfc3339()
}

// ─── Kampanyalar ──────────────────────────────────────────

fn map_campaign(row: &rusqlite::Row) -> rusqlite::Result<Campaign> {
    Ok(Campaign {
        id: row.get(0)?, name: row.get(1)?, discount_type: row.get(2)?,
        discount_value: row.get(3)?, days: row.get(4)?, start_time: row.get(5)?,
        end_time: row.get(6)?, active: row.get(7)?, created_at: row.get(8)?,
    })
}

fn norm_days(days: &str) -> String {
    let d = days.trim();
    if d.is_empty() || d == "all" { "all".to_string() }
    else { d.split(',').filter_map(|x| x.trim().parse::<i32>().ok()).filter(|x| (0..=6).contains(x)).collect::<Vec<_>>().iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",") }
}

fn parse_hhmm(t: &str) -> Option<(u32, u32)> {
    let mut it = t.split(':');
    let h: u32 = it.next()?.trim().parse().ok()?;
    let m: u32 = it.next()?.trim().parse().ok()?;
    Some((h, m))
}

pub fn campaign_discount_for(conn: &Connection, end: &DateTime<Local>) -> f64 {
    let weekday = match end.weekday() { Weekday::Sun => 0, Weekday::Mon => 1, Weekday::Tue => 2, Weekday::Wed => 3, Weekday::Thu => 4, Weekday::Fri => 5, Weekday::Sat => 6 };
    let mut stmt = match conn.prepare("SELECT name, discount_type, discount_value, days, start_time, end_time FROM campaigns WHERE active = 1") {
        Ok(s) => s, Err(_) => return 0.0,
    };
    let mut total_discount = 0.0;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, f64>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?)));
    if let Ok(rows) = rows {
        for row in rows.filter_map(|r| r.ok()) {
            let (_, dtype, dval, days, st, et) = row;
            let day_ok = days == "all" || days.split(',').any(|d| d == weekday.to_string());
            if !day_ok { continue; }
            let now_hhmm = (end.hour(), end.minute());
            let in_window = match (parse_hhmm(&st), parse_hhmm(&et)) {
                (Some((hs, ms)), Some((he, me))) => (now_hhmm.0, now_hhmm.1) >= (hs, ms) && (now_hhmm.0, now_hhmm.1) <= (he, me),
                _ => true,
            };
            if !in_window { continue; }
            let base = 100.0;
            let rate = if dtype == "percent" { dval.min(100.0) } else { dval };
            total_discount += base * rate / 100.0;
        }
    }
    total_discount
}

#[tauri::command]
pub fn list_campaigns(state: State<AppState>) -> Result<Vec<Campaign>, String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let mut stmt = conn.prepare("SELECT id, name, discount_type, discount_value, days, start_time, end_time, active, created_at FROM campaigns ORDER BY created_at DESC").map_err(|e| e.to_string())?;
    let items = stmt.query_map([], map_campaign).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).collect();
    Ok(items)
}

#[tauri::command]
pub fn add_campaign(name: String, discount_type: String, discount_value: f64, days: String, start_time: String, end_time: String, state: State<AppState>) -> Result<Campaign, String> {
    crate::commands::auth::require_admin(&state)?;
    if name.trim().is_empty() { return Err("Kampanya adı gerekli!".into()); }
    if discount_value <= 0.0 { return Err("İndirim değeri 0'dan büyük olmalı!".into()); }
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let id = Uuid::new_v4().to_string();
    let days = norm_days(&days);
    conn.execute("INSERT INTO campaigns (id, name, discount_type, discount_value, days, start_time, end_time, active, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,1,?8)",
        params![id, name, discount_type, discount_value, days, start_time, end_time, now()]).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "add_campaign", "campaigns", &name);
    conn.query_row("SELECT id, name, discount_type, discount_value, days, start_time, end_time, active, created_at FROM campaigns WHERE id = ?1", params![id], map_campaign).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_campaign(campaign_id: String, name: String, discount_type: String, discount_value: f64, days: String, start_time: String, end_time: String, active: bool, state: State<AppState>) -> Result<(), String> {
    crate::commands::auth::require_admin(&state)?;
    if name.trim().is_empty() { return Err("Kampanya adı gerekli!".into()); }
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let days = norm_days(&days);
    conn.execute("UPDATE campaigns SET name=?1, discount_type=?2, discount_value=?3, days=?4, start_time=?5, end_time=?6, active=?7 WHERE id=?8",
        params![name, discount_type, discount_value, days, start_time, end_time, if active {1} else {0}, campaign_id]).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "update_campaign", "campaigns", &name);
    Ok(())
}

#[tauri::command]
pub fn remove_campaign(campaign_id: String, state: State<AppState>) -> Result<(), String> {
    crate::commands::auth::require_admin(&state)?;
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    conn.execute("DELETE FROM campaigns WHERE id = ?1", params![campaign_id]).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "remove_campaign", "campaigns", &campaign_id);
    Ok(())
}

// ─── Paketler ─────────────────────────────────────────────

fn map_package(row: &rusqlite::Row) -> rusqlite::Result<Package> {
    Ok(Package {
        id: row.get(0)?, name: row.get(1)?, hours: row.get(2)?, price: row.get(3)?,
        description: row.get(4)?, active: row.get(5)?, created_at: row.get(6)?,
    })
}

#[tauri::command]
pub fn list_packages(state: State<AppState>) -> Result<Vec<Package>, String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let mut stmt = conn.prepare("SELECT id, name, hours, price, COALESCE(description,''), active, created_at FROM packages ORDER BY hours ASC").map_err(|e| e.to_string())?;
    let items = stmt.query_map([], map_package).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).collect();
    Ok(items)
}

#[tauri::command]
pub fn add_package(name: String, hours: i64, price: f64, description: String, state: State<AppState>) -> Result<Package, String> {
    crate::commands::auth::require_admin(&state)?;
    if name.trim().is_empty() { return Err("Paket adı gerekli!".into()); }
    if hours <= 0 { return Err("Saat 0'dan büyük olmalı!".into()); }
    if price <= 0.0 { return Err("Fiyat 0'dan büyük olmalı!".into()); }
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let id = Uuid::new_v4().to_string();
    conn.execute("INSERT INTO packages (id, name, hours, price, description, active, created_at) VALUES (?1,?2,?3,?4,?5,1,?6)",
        params![id, name, hours, price, description, now()]).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "add_package", "packages", &name);
    conn.query_row("SELECT id, name, hours, price, COALESCE(description,''), active, created_at FROM packages WHERE id = ?1", params![id], map_package).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_package(package_id: String, name: String, hours: i64, price: f64, description: String, active: bool, state: State<AppState>) -> Result<(), String> {
    crate::commands::auth::require_admin(&state)?;
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    conn.execute("UPDATE packages SET name=?1, hours=?2, price=?3, description=?4, active=?5 WHERE id=?6",
        params![name, hours, price, description, if active {1} else {0}, package_id]).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "update_package", "packages", &name);
    Ok(())
}

#[tauri::command]
pub fn remove_package(package_id: String, state: State<AppState>) -> Result<(), String> {
    crate::commands::auth::require_admin(&state)?;
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    conn.execute("DELETE FROM packages WHERE id = ?1", params![package_id]).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "remove_package", "packages", &package_id);
    Ok(())
}

// ─── Promosyon Kodları ────────────────────────────────────

fn map_promo(row: &rusqlite::Row) -> rusqlite::Result<PromoCode> {
    Ok(PromoCode {
        id: row.get(0)?, code: row.get(1)?, discount_type: row.get(2)?,
        discount_value: row.get(3)?, max_uses: row.get(4)?, used_count: row.get(5)?,
        active: row.get(6)?, valid_from: row.get(7)?, valid_until: row.get(8)?, created_at: row.get(9)?,
    })
}

pub fn validate_promo(conn: &Connection, code: &str) -> Result<PromoCode, String> {
    let pc: PromoCode = conn.query_row("SELECT id, code, discount_type, discount_value, max_uses, used_count, active, valid_from, valid_until, created_at FROM promo_codes WHERE code = ?1", params![code.trim()], map_promo).map_err(|_| "Promosyon kodu bulunamadı!")?;
    if pc.active != 1 { return Err("Bu promosyon kodu pasif!".into()); }
    if pc.max_uses > 0 && pc.used_count >= pc.max_uses { return Err("Bu promosyon kodu kullanım limitine ulaştı!".into()); }
    if let Some(vf) = &pc.valid_from { if now() < *vf { return Err("Bu kod henüz geçerli değil!".into()); } }
    if let Some(vu) = &pc.valid_until { if now() > *vu { return Err("Bu kodun süresi dolmuş!".into()); } }
    Ok(pc)
}

pub fn promo_discount_amount(pc: &PromoCode, total: f64) -> f64 {
    if pc.discount_type == "percent" {
        total * pc.discount_value.min(100.0) / 100.0
    } else {
        pc.discount_value.min(total)
    }
}

#[tauri::command]
pub fn list_promo_codes(state: State<AppState>) -> Result<Vec<PromoCode>, String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let mut stmt = conn.prepare("SELECT id, code, discount_type, discount_value, max_uses, used_count, active, valid_from, valid_until, created_at FROM promo_codes ORDER BY created_at DESC").map_err(|e| e.to_string())?;
    let items = stmt.query_map([], map_promo).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).collect();
    Ok(items)
}

#[tauri::command]
pub fn add_promo_code(code: String, discount_type: String, discount_value: f64, max_uses: i64, valid_until: Option<String>, state: State<AppState>) -> Result<PromoCode, String> {
    crate::commands::auth::require_admin(&state)?;
    let code = code.trim().to_uppercase();
    if code.len() < 2 { return Err("Kod en az 2 karakter olmalı!".into()); }
    if discount_value <= 0.0 { return Err("İndirim değeri 0'dan büyük olmalı!".into()); }
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let exists: bool = conn.query_row("SELECT COUNT(*) FROM promo_codes WHERE code = ?1", params![code], |r| r.get::<_, i64>(0)).unwrap_or(0) > 0;
    if exists { return Err("Bu kod zaten mevcut!".into()); }
    let id = Uuid::new_v4().to_string();
    let vu = valid_until.filter(|v| !v.trim().is_empty());
    conn.execute("INSERT INTO promo_codes (id, code, discount_type, discount_value, max_uses, used_count, active, valid_from, valid_until, created_at) VALUES (?1,?2,?3,?4,?5,0,1,NULL,?6,?7)",
        params![id, code, discount_type, discount_value, max_uses.max(0), vu, now()]).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "add_promo_code", "promo_codes", &code);
    conn.query_row("SELECT id, code, discount_type, discount_value, max_uses, used_count, active, valid_from, valid_until, created_at FROM promo_codes WHERE id = ?1", params![id], map_promo).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_promo_code(promo_id: String, code: String, discount_type: String, discount_value: f64, max_uses: i64, valid_until: Option<String>, active: bool, state: State<AppState>) -> Result<(), String> {
    crate::commands::auth::require_admin(&state)?;
    let code = code.trim().to_uppercase();
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let vu = valid_until.filter(|v| !v.trim().is_empty());
    conn.execute("UPDATE promo_codes SET code=?1, discount_type=?2, discount_value=?3, max_uses=?4, valid_until=?5, active=?6 WHERE id=?7",
        params![code, discount_type, discount_value, max_uses.max(0), vu, if active {1} else {0}, promo_id]).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "update_promo_code", "promo_codes", &code);
    Ok(())
}

#[tauri::command]
pub fn remove_promo_code(promo_id: String, state: State<AppState>) -> Result<(), String> {
    crate::commands::auth::require_admin(&state)?;
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    conn.execute("DELETE FROM promo_codes WHERE id = ?1", params![promo_id]).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "remove_promo_code", "promo_codes", &promo_id);
    Ok(())
}

#[tauri::command]
pub fn validate_promo_code(code: String, state: State<AppState>) -> Result<PromoCode, String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    validate_promo(&conn, &code)
}
