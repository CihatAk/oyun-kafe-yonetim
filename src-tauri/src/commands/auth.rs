use rusqlite::params;
use tauri::State;
use uuid::Uuid;
use chrono::Local;

use crate::db::{AppState, CurrentUser};
use crate::models::*;

const SALT: &str = "oyun-kafe-2026";

pub fn hash_password(password: &str, salt: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(salt.as_bytes());
    hasher.update(password.as_bytes());
    let result = hasher.finalize();
    result.iter().map(|b| format!("{:02x}", b)).collect()
}

fn now() -> String {
    Local::now().to_rfc3339()
}

fn get_current(state: &AppState) -> Option<CurrentUser> {
    state.current_user.lock().ok().and_then(|g| g.clone())
}

pub fn require_admin(state: &State<AppState>) -> Result<CurrentUser, String> {
    let u = get_current(state).ok_or("Giriş yapılmamış!")?;
    if u.role != "admin" { return Err("Bu işlem için yönetici yetkisi gerekli!".into()); }
    Ok(u)
}

pub fn log_audit(state: &State<AppState>, action: &str, entity: &str, detail: &str) {
    if let Ok(conn) = state.db.lock() {
        log_audit_conn(&conn, state, action, entity, detail);
    }
}

pub fn log_audit_conn(conn: &rusqlite::Connection, state: &AppState, action: &str, entity: &str, detail: &str) {
    let user = state.current_user.lock().ok().and_then(|g| g.clone());
    let id = Uuid::new_v4().to_string();
    let (uid, uname) = match &user {
        Some(u) => (u.id.clone(), u.full_name.clone()),
        None => (String::new(), String::from("Sistem")),
    };
    let uname = if uname.is_empty() { user.as_ref().map(|u| u.username.clone()).unwrap_or_else(|| String::from("Sistem")) } else { uname };
    let _ = conn.execute("INSERT INTO audit_log (id, user_id, user_name, action, entity, detail, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![id, uid, uname, action, entity, detail, now()]);
}

#[tauri::command]
pub fn login(username: String, password: String, state: State<AppState>) -> Result<CurrentUser, String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let user = conn.query_row("SELECT id, username, password_hash, full_name, role, active FROM users WHERE username = ?1", params![username],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, i64>(5)?)))
        .map_err(|_| "Kullanıcı bulunamadı!")?;
    let (id, uname, hash, full_name, role, active) = user;
    if active != 1 { return Err("Kullanıcı pasif!".into()); }
    if hash != hash_password(&password, SALT) { return Err("Yanlış şifre!".into()); }
    let cu = CurrentUser { id, username: uname, full_name, role };
    *state.current_user.lock().map_err(|e| e.to_string())? = Some(cu.clone());
    Ok(cu)
}

#[tauri::command]
pub fn logout(state: State<AppState>) -> Result<(), String> {
    log_audit(&state, "logout", "auth", "Çıkış yapıldı");
    *state.current_user.lock().map_err(|e| e.to_string())? = None;
    Ok(())
}

#[tauri::command]
pub fn get_current_user(state: State<AppState>) -> Result<Option<CurrentUser>, String> {
    Ok(get_current(&state))
}

#[tauri::command]
pub fn list_users(state: State<AppState>) -> Result<Vec<UserRecord>, String> {
    require_admin(&state)?;
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let mut stmt = conn.prepare("SELECT id, username, full_name, role, active, created_at FROM users ORDER BY role DESC, full_name").map_err(|e| e.to_string())?;
    let users = stmt.query_map([], |row| Ok(UserRecord { id: row.get(0)?, username: row.get(1)?, full_name: row.get(2)?, role: row.get(3)?, active: row.get(4)?, created_at: row.get(5)? })).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).collect();
    Ok(users)
}

#[tauri::command]
pub fn add_user(username: String, password: String, full_name: String, role: String, state: State<AppState>) -> Result<(), String> {
    require_admin(&state)?;
    if username.trim().len() < 3 { return Err("Kullanıcı adı en az 3 karakter olmalı!".into()); }
    if password.len() < 4 { return Err("Şifre en az 4 karakter olmalı!".into()); }
    let role = if role == "admin" { "admin" } else { "calisan" };
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let exists: bool = conn.query_row("SELECT COUNT(*) FROM users WHERE username = ?1", params![username.trim()], |r| r.get::<_, i64>(0)).unwrap_or(0) > 0;
    if exists { return Err("Bu kullanıcı adı zaten var!".into()); }
    let id = Uuid::new_v4().to_string();
    conn.execute("INSERT INTO users (id, username, password_hash, full_name, role, active, created_at) VALUES (?1,?2,?3,?4,?5,1,?6)",
        params![id, username.trim(), hash_password(&password, SALT), full_name, role, now()]).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "add_user", "users", format!("{} eklendi ({})", full_name, role).as_str());
    Ok(())
}

#[tauri::command]
pub fn update_user(user_id: String, full_name: String, role: String, active: bool, state: State<AppState>) -> Result<(), String> {
    let admin = require_admin(&state)?;
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    if user_id == admin.id && active == false { return Err("Kendi hesabınızı kapatamazsınız!".into()); }
    let role = if role == "admin" { "admin" } else { "calisan" };
    conn.execute("UPDATE users SET full_name = ?1, role = ?2, active = ?3 WHERE id = ?4", params![full_name, role, if active {1} else {0}, user_id]).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "update_user", "users", format!("{} güncellendi", full_name).as_str());
    Ok(())
}

#[tauri::command]
pub fn remove_user(user_id: String, state: State<AppState>) -> Result<(), String> {
    let admin = require_admin(&state)?;
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    if user_id == admin.id { return Err("Kendi hesabınızı silemezsiniz!".into()); }
    conn.execute("DELETE FROM users WHERE id = ?1", params![user_id]).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "remove_user", "users", "Kullanıcı silindi");
    Ok(())
}

#[tauri::command]
pub fn change_password(old_password: String, new_password: String, state: State<AppState>) -> Result<(), String> {
    let user = get_current(&state).ok_or("Giriş yapılmamış!")?;
    if new_password.len() < 4 { return Err("Yeni şifre en az 4 karakter olmalı!".into()); }
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let current_hash: String = conn.query_row("SELECT password_hash FROM users WHERE id = ?1", params![user.id], |r| r.get(0)).map_err(|_| "Kullanıcı bulunamadı!")?;
    if current_hash != hash_password(&old_password, SALT) { return Err("Mevcut şifre yanlış!".into()); }
    conn.execute("UPDATE users SET password_hash = ?1 WHERE id = ?2", params![hash_password(&new_password, SALT), user.id]).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "change_password", "auth", "Şifre değiştirildi");
    Ok(())
}

#[tauri::command]
pub fn clock_in(state: State<AppState>) -> Result<ShiftRecord, String> {
    let user = get_current(&state).ok_or("Giriş yapılmamış!")?;
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let open: i64 = conn.query_row("SELECT COUNT(*) FROM shifts WHERE user_id = ?1 AND status = 'open'", params![user.id], |r| r.get(0)).unwrap_or(0);
    if open > 0 { return Err("Zaten açık vardiyanız var!".into()); }
    let id = Uuid::new_v4().to_string();
    let uname = if user.full_name.is_empty() { user.username.clone() } else { user.full_name.clone() };
    conn.execute("INSERT INTO shifts (id, user_id, user_name, start_time, status) VALUES (?1,?2,?3,?4,'open')", params![id, user.id, uname, now()]).map_err(|e| e.to_string())?;
    let record = conn.query_row("SELECT id, user_id, user_name, start_time, end_time, status, total_sessions, total_revenue FROM shifts WHERE id = ?1", params![id],
        |r| Ok(ShiftRecord { id: r.get(0)?, user_id: r.get(1)?, user_name: r.get(2)?, start_time: r.get(3)?, end_time: r.get(4)?, status: r.get(5)?, total_sessions: r.get(6)?, total_revenue: r.get(7)? })).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "clock_in", "shifts", "Vardiya başlatıldı");
    Ok(record)
}

#[tauri::command]
pub fn clock_out(state: State<AppState>) -> Result<ShiftRecord, String> {
    let user = get_current(&state).ok_or("Giriş yapılmamış!")?;
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let mut stmt = conn.prepare("SELECT id, user_id, user_name, start_time, end_time, status, total_sessions, total_revenue FROM shifts WHERE user_id = ?1 AND status = 'open' ORDER BY start_time DESC LIMIT 1").map_err(|e| e.to_string())?;
    let shift = stmt.query_row(params![user.id], |r| Ok(ShiftRecord { id: r.get(0)?, user_id: r.get(1)?, user_name: r.get(2)?, start_time: r.get(3)?, end_time: r.get(4)?, status: r.get(5)?, total_sessions: r.get(6)?, total_revenue: r.get(7)? })).map_err(|_| "Açık vardiya bulunamadı!")?;
    conn.execute("UPDATE shifts SET end_time = ?1, status = 'closed' WHERE id = ?2", params![now(), shift.id]).map_err(|e| e.to_string())?;
    let updated = conn.query_row("SELECT id, user_id, user_name, start_time, end_time, status, total_sessions, total_revenue FROM shifts WHERE id = ?1", params![shift.id],
        |r| Ok(ShiftRecord { id: r.get(0)?, user_id: r.get(1)?, user_name: r.get(2)?, start_time: r.get(3)?, end_time: r.get(4)?, status: r.get(5)?, total_sessions: r.get(6)?, total_revenue: r.get(7)? })).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "clock_out", "shifts", "Vardiya kapatıldı");
    Ok(updated)
}

#[tauri::command]
pub fn get_active_shift(state: State<AppState>) -> Result<Option<ShiftRecord>, String> {
    let user = get_current(&state).ok_or("Giriş yapılmamış!")?;
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let mut stmt = conn.prepare("SELECT id, user_id, user_name, start_time, end_time, status, total_sessions, total_revenue FROM shifts WHERE user_id = ?1 AND status = 'open' ORDER BY start_time DESC LIMIT 1").map_err(|e| e.to_string())?;
    let result = stmt.query_row(params![user.id], |r| Ok(ShiftRecord { id: r.get(0)?, user_id: r.get(1)?, user_name: r.get(2)?, start_time: r.get(3)?, end_time: r.get(4)?, status: r.get(5)?, total_sessions: r.get(6)?, total_revenue: r.get(7)? })).ok();
    Ok(result)
}

#[tauri::command]
pub fn get_shift_history(state: State<AppState>) -> Result<Vec<ShiftRecord>, String> {
    let user = get_current(&state).ok_or("Giriş yapılmamış!")?;
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let mut stmt = if user.role == "admin" {
        conn.prepare("SELECT id, user_id, user_name, start_time, end_time, status, total_sessions, total_revenue FROM shifts ORDER BY start_time DESC LIMIT 200").map_err(|e| e.to_string())?
    } else {
        conn.prepare("SELECT id, user_id, user_name, start_time, end_time, status, total_sessions, total_revenue FROM shifts WHERE user_id = ?1 ORDER BY start_time DESC LIMIT 100").map_err(|e| e.to_string())?
    };
    let shifts = if user.role == "admin" {
        stmt.query_map([], |r| Ok(ShiftRecord { id: r.get(0)?, user_id: r.get(1)?, user_name: r.get(2)?, start_time: r.get(3)?, end_time: r.get(4)?, status: r.get(5)?, total_sessions: r.get(6)?, total_revenue: r.get(7)? })).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).collect()
    } else {
        stmt.query_map(params![user.id], |r| Ok(ShiftRecord { id: r.get(0)?, user_id: r.get(1)?, user_name: r.get(2)?, start_time: r.get(3)?, end_time: r.get(4)?, status: r.get(5)?, total_sessions: r.get(6)?, total_revenue: r.get(7)? })).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).collect()
    };
    Ok(shifts)
}

#[tauri::command]
pub fn reset_user_password(user_id: String, new_password: String, state: State<AppState>) -> Result<(), String> {
    require_admin(&state)?;
    if new_password.len() < 4 { return Err("Yeni şifre en az 4 karakter olmalı!".into()); }
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    conn.execute("UPDATE users SET password_hash = ?1 WHERE id = ?2", params![hash_password(&new_password, SALT), user_id]).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "reset_password", "users", "Şifre sıfırlandı");
    Ok(())
}

#[tauri::command]
pub fn get_audit_log(state: State<AppState>, limit: Option<i64>) -> Result<Vec<AuditRecord>, String> {
    require_admin(&state)?;
    let lim = limit.unwrap_or(200);
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let mut stmt = conn.prepare("SELECT id, user_name, action, entity, detail, created_at FROM audit_log ORDER BY created_at DESC LIMIT ?1").map_err(|e| e.to_string())?;
    let logs = stmt.query_map(params![lim], |r| Ok(AuditRecord { id: r.get(0)?, user_name: r.get(1)?, action: r.get(2)?, entity: r.get(3)?, detail: r.get(4)?, created_at: r.get(5)? })).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).collect();
    Ok(logs)
}
