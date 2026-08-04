use rusqlite::params;
use tauri::State;
use uuid::Uuid;
use chrono::Local;
use std::collections::HashMap;
use std::sync::Mutex;

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;

use crate::db::{AppState, CurrentUser};
use crate::models::*;

const LEGACY_SALT: &str = "oyun-kafe-2026";
const MAX_FAILED_ATTEMPTS: usize = 5;
const LOCKOUT_SECONDS: i64 = 300;

static LOGIN_FAILURES: Mutex<Option<HashMap<String, Vec<i64>>>> = Mutex::new(None);

fn check_lockout(username: &str) -> Result<(), String> {
    let now = Local::now().timestamp();
    let mut g = LOGIN_FAILURES.lock().map_err(|_| "Kilit durumu okunamadı".to_string())?;
    let map = g.get_or_insert_with(HashMap::new);
    let entry = map.entry(username.to_string()).or_default();
    entry.retain(|t| now - t < LOCKOUT_SECONDS);
    if entry.len() >= MAX_FAILED_ATTEMPTS {
        let wait = entry[0] + LOCKOUT_SECONDS - now;
        return Err(format!("Çok fazla hatalı deneme. {} sn sonra tekrar deneyin.", wait.max(0)));
    }
    Ok(())
}

fn record_failure(username: &str) {
    let now = Local::now().timestamp();
    if let Ok(mut g) = LOGIN_FAILURES.lock() {
        let map = g.get_or_insert_with(HashMap::new);
        map.entry(username.to_string()).or_default().push(now);
    }
}

fn clear_failures(username: &str) {
    if let Ok(mut g) = LOGIN_FAILURES.lock() {
        if let Some(map) = g.as_mut() {
            map.remove(username);
        }
    }
}

pub fn make_hash(password: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    match Argon2::default().hash_password(password.as_bytes(), &salt) {
        Ok(h) => h.to_string(),
        Err(_) => legacy_hash(password),
    }
}

pub fn verify_password(password: &str, stored: &str) -> bool {
    if stored.starts_with("$argon2") {
        if let Ok(parsed) = PasswordHash::new(stored) {
            return Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok();
        }
        return false;
    }
    legacy_hash(password) == stored
}

fn legacy_hash(password: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(LEGACY_SALT.as_bytes());
    hasher.update(password.as_bytes());
    let result = hasher.finalize();
    result.iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_and_verify_roundtrip() {
        let h = make_hash("sifre123");
        assert!(h.starts_with("$argon2"), "argon2 format olmali");
        assert!(verify_password("sifre123", &h));
        assert!(!verify_password("yanlis", &h));
    }

    #[test]
    fn legacy_hash_compat() {
        let old = legacy_hash("admin123");
        assert_eq!(old.len(), 64);
        assert!(verify_password("admin123", &old));
        assert!(!verify_password("yanlis", &old));
    }
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

pub fn require_login(state: &State<AppState>) -> Result<CurrentUser, String> {
    get_current(state).ok_or_else(|| "Giriş yapılmamış!".to_string())
}

pub fn permissions_json(discount_limit: Option<f64>) -> String {
    let limit = discount_limit.unwrap_or(0.0).max(0.0);
    serde_json::json!({ "discount_limit": limit }).to_string()
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
    check_lockout(&username)?;
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let user = conn.query_row("SELECT id, username, password_hash, full_name, role, active, COALESCE(permissions,'{}'), COALESCE(must_change_password,0) FROM users WHERE username = ?1", params![username],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, i64>(5)?, r.get::<_, String>(6)?, r.get::<_, i64>(7)?)))
        .map_err(|_| { record_failure(&username); "Kullanıcı bulunamadı!".to_string() })?;
    let (id, uname, hash, full_name, role, active, permissions, must_change_password) = user;
    if active != 1 { record_failure(&username); return Err("Kullanıcı pasif!".into()); }
    if !verify_password(&password, &hash) { record_failure(&username); return Err("Yanlış şifre!".into()); }
    clear_failures(&username);
    let cu = CurrentUser { id, username: uname, full_name, role, permissions, must_change_password: must_change_password == 1 };
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
    let mut stmt = conn.prepare("SELECT id, username, full_name, role, active, COALESCE(permissions,'{}'), created_at FROM users ORDER BY role DESC, full_name").map_err(|e| e.to_string())?;
    let users = stmt.query_map([], |row| Ok(UserRecord { id: row.get(0)?, username: row.get(1)?, full_name: row.get(2)?, role: row.get(3)?, active: row.get(4)?, permissions: row.get(5)?, created_at: row.get(6)? })).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).collect();
    Ok(users)
}

#[tauri::command]
pub fn add_user(username: String, password: String, full_name: String, role: String, discount_limit: Option<f64>, state: State<AppState>) -> Result<(), String> {
    require_admin(&state)?;
    if username.trim().len() < 3 { return Err("Kullanıcı adı en az 3 karakter olmalı!".into()); }
    if password.len() < 6 { return Err("Şifre en az 6 karakter olmalı!".into()); }
    let role = if role == "admin" { "admin" } else { "calisan" };
    let permissions = permissions_json(discount_limit);
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let exists: bool = conn.query_row("SELECT COUNT(*) FROM users WHERE username = ?1", params![username.trim()], |r| r.get::<_, i64>(0)).unwrap_or(0) > 0;
    if exists { return Err("Bu kullanıcı adı zaten var!".into()); }
    let id = Uuid::new_v4().to_string();
    conn.execute("INSERT INTO users (id, username, password_hash, full_name, role, active, permissions, created_at) VALUES (?1,?2,?3,?4,?5,1,?6,?7)",
        params![id, username.trim(), make_hash(&password), full_name, role, permissions, now()]).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "add_user", "users", format!("{} eklendi ({})", full_name, role).as_str());
    Ok(())
}

#[tauri::command]
pub fn update_user(user_id: String, full_name: String, role: String, active: bool, discount_limit: Option<f64>, state: State<AppState>) -> Result<(), String> {
    let admin = require_admin(&state)?;
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    if user_id == admin.id && active == false { return Err("Kendi hesabınızı kapatamazsınız!".into()); }
    let role = if role == "admin" { "admin" } else { "calisan" };
    let permissions = permissions_json(discount_limit);
    conn.execute("UPDATE users SET full_name = ?1, role = ?2, active = ?3, permissions = ?4 WHERE id = ?5", params![full_name, role, if active {1} else {0}, permissions, user_id]).map_err(|e| e.to_string())?;
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
    if new_password.len() < 6 { return Err("Yeni şifre en az 6 karakter olmalı!".into()); }
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let current_hash: String = conn.query_row("SELECT password_hash FROM users WHERE id = ?1", params![user.id], |r| r.get(0)).map_err(|_| "Kullanıcı bulunamadı!")?;
    if !verify_password(&old_password, &current_hash) { return Err("Mevcut şifre yanlış!".into()); }
    conn.execute("UPDATE users SET password_hash = ?1, must_change_password = 0 WHERE id = ?2", params![make_hash(&new_password), user.id]).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "change_password", "auth", "Şifre değiştirildi");
    Ok(())
}

#[tauri::command]
pub fn force_change_password(new_password: String, state: State<AppState>) -> Result<(), String> {
    let user = get_current(&state).ok_or("Giriş yapılmamış!")?;
    if new_password.len() < 6 { return Err("Yeni şifre en az 6 karakter olmalı!".into()); }
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let must: i64 = conn.query_row("SELECT COALESCE(must_change_password,0) FROM users WHERE id = ?1", params![user.id], |r| r.get(0)).unwrap_or(0);
    if must != 1 { return Err("Şifre değişikliği zorunlu değil".into()); }
    conn.execute("UPDATE users SET password_hash = ?1, must_change_password = 0 WHERE id = ?2", params![make_hash(&new_password), user.id]).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "change_password", "auth", "İlk girişte şifre değiştirildi");
    Ok(())
}

#[tauri::command]
pub fn reset_user_password(user_id: String, new_password: String, state: State<AppState>) -> Result<(), String> {
    require_admin(&state)?;
    if new_password.len() < 6 { return Err("Yeni şifre en az 6 karakter olmalı!".into()); }
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    conn.execute("UPDATE users SET password_hash = ?1, must_change_password = 0 WHERE id = ?2", params![make_hash(&new_password), user_id]).map_err(|e| e.to_string())?;
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
