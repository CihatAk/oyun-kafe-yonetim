use std::fs;
use std::time::Duration;

use chrono::{DateTime, Local};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rusqlite::Connection;
use serde_json::{json, Value};
use tauri::State;

use crate::commands::settings::{get_setting_value, set_setting_value};
use crate::db::AppState;

pub const TRIAL_DAYS: i64 = 15;

// Lisans sunucusu yapılandırması (build sabiti).
// Lütfen kendi Supabase projenizin değerlerini girin.
const LICENSE_SERVER_URL: &str = "https://YOUR-PROJECT-REF.supabase.co";
const LICENSE_ANON_KEY: &str = "";

// Ed25519 doğrulama anahtarı (32 bayt, hex).
// GİZLİ: Özel anahtar yalnızca lisans sunucusunda (Supabase edge function env) tutulur.
// scripts/gen-license-keys.mjs ile üretilen public key buraya yazılır.
pub const DEFAULT_PUBLIC_KEY_HEX: &str = "";

fn public_key() -> Result<VerifyingKey, String> {
    let hex = std::env::var("LICENSE_PUBLIC_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_PUBLIC_KEY_HEX.to_string());
    if hex.trim().is_empty() {
        return Err("Lisans doğrulama anahtarı yapılandırılmamış.".into());
    }
    let bytes = hex_to_bytes(&hex).ok_or("Lisans anahtarı geçersiz hex.")?;
    if bytes.len() != 32 {
        return Err("Lisans anahtarı 32 bayt olmalı.".into());
    }
    VerifyingKey::from_bytes(&bytes[..].try_into().map_err(|_| "anahtar boyutu".to_string())?)
        .map_err(|e| format!("Lisans anahtarı okunamadı: {}", e))
}

fn license_server_url() -> Option<String> {
    std::env::var("LICENSE_SERVER_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            let v = LICENSE_SERVER_URL.trim();
            if v.is_empty() || v.starts_with("https://YOUR-") { None } else { Some(v.to_string()) }
        })
}

fn license_anon_key() -> Option<String> {
    if let Ok(v) = std::env::var("LICENSE_ANON_KEY") {
        if !v.trim().is_empty() {
            return Some(v.trim().to_string());
        }
    }
    if !LICENSE_ANON_KEY.trim().is_empty() {
        return Some(LICENSE_ANON_KEY.trim().to_string());
    }
    fs::read_to_string(crate::db::get_data_dir().join("supabase.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|v| v["anon_key"].as_str().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
}

// ─── Makine kodu ────────────────────────────────────────────

fn machine_guid() -> String {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    hklm.open_subkey(r"SOFTWARE\Microsoft\Cryptography")
        .ok()
        .and_then(|k| k.get_value::<String, _>("MachineGuid").ok())
        .unwrap_or_default()
}

pub fn machine_id() -> String {
    let guid = machine_guid();
    if guid.is_empty() {
        // Kayıt defteri okunamazsa kalıcı rastgele id (data dizininde saklanır).
        let path = crate::db::get_data_dir().join(".machine-id");
        if let Ok(raw) = fs::read_to_string(&path) {
            if !raw.trim().is_empty() {
                return raw.trim().to_string();
            }
        }
        let id = uuid::Uuid::new_v4().to_string();
        let _ = fs::create_dir_all(crate::db::get_data_dir());
        let _ = fs::write(&path, &id);
        return id;
    }
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update("oyun-kafe-lic-v1");
    hasher.update(guid.as_bytes());
    let result = hasher.finalize();
    result.iter().map(|b| format!("{:02x}", b)).collect()
}

// ─── Token doğrulama ────────────────────────────────────────

fn hex_to_bytes(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn encode_token(payload: &Value, sk: &SigningKey) -> String {
    let msg = payload.to_string();
    let sig = sk.sign(msg.as_bytes());
    format!("{}.{}", bytes_to_hex(msg.as_bytes()), bytes_to_hex(&sig.to_bytes()))
}

fn parse_token_with(token: &str, pk: &VerifyingKey) -> Result<Value, String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 2 {
        return Err("Lisans token'ı bozuk.".into());
    }
    let msg_bytes = hex_to_bytes(parts[0]).ok_or("Lisans token'ı bozuk (payload).")?;
    let sig_bytes = hex_to_bytes(parts[1]).ok_or("Lisans token'ı bozuk (imza).")?;
    if sig_bytes.len() != 64 {
        return Err("Lisans imzası geçersiz.".into());
    }
    let sig = Signature::from_bytes(
        sig_bytes[..]
            .try_into()
            .map_err(|_| "imza boyutu".to_string())?,
    );
    pk.verify(&msg_bytes, &sig)
        .map_err(|_| "Lisans imzası doğrulanamadı.".to_string())?;
    let payload: Value =
        serde_json::from_slice(&msg_bytes).map_err(|_| "Lisans içeriği okunamadı.".to_string())?;
    if payload.get("v").and_then(|v| v.as_i64()) != Some(1) {
        return Err("Desteklenmeyen lisans sürümü.".into());
    }
    Ok(payload)
}

fn parse_token(token: &str) -> Result<Value, String> {
    let pk = public_key()?;
    parse_token_with(token, &pk)
}

fn is_expired(payload: &Value) -> bool {
    let Some(exp) = payload.get("expires_at").and_then(|v| v.as_str()) else {
        return false; // süresiz (ömür boyu)
    };
    match DateTime::parse_from_rfc3339(exp) {
        Ok(dt) => Local::now() > dt.with_timezone(&Local),
        Err(_) => false,
    }
}

fn elapsed_trial_days(start: DateTime<Local>) -> i64 {
    (Local::now() - start).num_days()
}

// ─── Durum ──────────────────────────────────────────────────

#[derive(Clone, serde::Serialize)]
pub struct LicenseStatus {
    pub state: String, // licensed | trial | expired | locked
    pub license_id: Option<String>,
    pub business_name: Option<String>,
    pub trial_days_left: Option<i64>,
    pub message: Option<String>,
}

fn licensed_status(payload: &Value) -> LicenseStatus {
    LicenseStatus {
        state: "licensed".into(),
        license_id: payload.get("license_id").and_then(|v| v.as_str()).map(String::from),
        business_name: payload.get("business_name").and_then(|v| v.as_str()).map(String::from),
        trial_days_left: None,
        message: None,
    }
}

#[tauri::command]
pub fn get_license_status(state: State<AppState>) -> Result<LicenseStatus, String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    if let Some(token) = get_setting_value(&conn, "license_token") {
        let payload = match parse_token(&token) {
            Ok(p) => p,
            Err(e) => {
                return Ok(LicenseStatus {
                    state: "locked".into(),
                    license_id: None,
                    business_name: None,
                    trial_days_left: None,
                    message: Some(e),
                })
            }
        };
        if payload.get("machine_hash").and_then(|v| v.as_str()) != Some(machine_id().as_str()) {
            return Ok(LicenseStatus {
                state: "locked".into(),
                license_id: None,
                business_name: None,
                trial_days_left: None,
                message: Some("Bu lisans farklı bir bilgisayara bağlı.".into()),
            });
        }
        if is_revoked(&conn) {
            return Ok(LicenseStatus {
                state: "locked".into(),
                license_id: payload.get("license_id").and_then(|v| v.as_str()).map(String::from),
                business_name: payload.get("business_name").and_then(|v| v.as_str()).map(String::from),
                trial_days_left: None,
                message: Some("Bu lisans iptal edilmiştir. Satıcıyla iletişime geçin.".into()),
            });
        }
        if is_expired(&payload) {
            return Ok(LicenseStatus {
                state: "expired".into(),
                license_id: payload.get("license_id").and_then(|v| v.as_str()).map(String::from),
                business_name: payload.get("business_name").and_then(|v| v.as_str()).map(String::from),
                trial_days_left: None,
                message: Some("Lisans süresi doldu.".into()),
            });
        }
        return Ok(licensed_status(&payload));
    }

    // Deneme modu
    let start_str = get_setting_value(&conn, "trial_started_at");
    let start = match start_str.and_then(|s| DateTime::parse_from_rfc3339(&s).ok()) {
        Some(dt) => dt.with_timezone(&Local),
        None => {
            let now = Local::now();
            let _ = set_setting_value(&conn, "trial_started_at", &now.to_rfc3339());
            now
        }
    };
    let days = elapsed_trial_days(start);
    if days >= TRIAL_DAYS {
        return Ok(LicenseStatus {
            state: "expired".into(),
            license_id: None,
            business_name: None,
            trial_days_left: Some(0),
            message: Some("Deneme süresi doldu. Lisans anahtarı girin.".into()),
        });
    }
    Ok(LicenseStatus {
        state: "trial".into(),
        license_id: None,
        business_name: None,
        trial_days_left: Some(TRIAL_DAYS - days),
        message: None,
    })
}

// ─── Aktivasyon ─────────────────────────────────────────────

#[tauri::command]
pub async fn activate_license(key: String, state: State<'_, AppState>) -> Result<LicenseStatus, String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("Lisans anahtarı boş olamaz.".into());
    }
    let machine = machine_id();
    let server = license_server_url().ok_or("Lisans sunucusu yapılandırılmamış.")?;
    let anon = license_anon_key().ok_or("Lisans sunucusu anahtarı bulunamadı.")?;
    let url = format!("{}/functions/v1/license/activate", server.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", anon))
        .header("apikey", &anon)
        .json(&json!({ "key": key, "machine_hash": machine }))
        .send()
        .await
        .map_err(|e| format!("Lisans sunucusuna ulaşılamadı: {}", e))?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|_| json!({})).unwrap_or_default();
    if !status.is_success() {
        let msg = body
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Lisans sunucusu hatası.");
        return Err(msg.to_string());
    }
    let token = body
        .get("token")
        .and_then(|v| v.as_str())
        .ok_or("Sunucu lisans token'ı döndürmedi.")?;
    let payload = parse_token(token)?;
    if payload.get("machine_hash").and_then(|v| v.as_str()) != Some(machine.as_str()) {
        return Err("Lisans bu bilgisayara bağlanmadı.".into());
    }
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    set_setting_value(&conn, "license_token", token)?;
    Ok(licensed_status(&payload))
}

// ─── Online kontrol (iptal/geçerlilik) ──────────────────────

fn is_revoked(conn: &Connection) -> bool {
    get_setting_value(conn, "license_revoked").map(|v| v == "1").unwrap_or(false)
}

fn mark_revoked(conn: &Connection) {
    let _ = set_setting_value(conn, "license_revoked", "1");
}

fn clear_revoked(conn: &Connection) {
    let _ = set_setting_value(conn, "license_revoked", "0");
}

/// Lisans sunucusuna tek seferlik kontrol. İptal edilmişse bayrağı işaretler.
/// Ağ hatasında sessizce geçer (offline çalışma korunur).
pub fn check_license_online() -> Result<(), String> {
    let conn = Connection::open(crate::db::get_db_path()).map_err(|e| e.to_string())?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
        .map_err(|e| e.to_string())?;
    let token = match get_setting_value(&conn, "license_token") {
        Some(t) => t,
        None => return Ok(()),
    };
    let payload = match parse_token(&token) {
        Ok(p) => p,
        Err(_) => return Ok(()),
    };
    let license_id = match payload.get("license_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return Ok(()),
    };
    let server = match license_server_url() {
        Some(s) => s,
        None => return Ok(()),
    };
    let anon = match license_anon_key() {
        Some(k) => k,
        None => return Ok(()),
    };
    let url = format!("{}/functions/v1/license/check", server.trim_end_matches('/'));
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", anon))
        .header("apikey", &anon)
        .json(&json!({ "license_id": license_id, "machine_hash": machine_id() }))
        .send()
        .map_err(|e| format!("kontrol edilemedi: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("sunucu hatası: {}", resp.status()));
    }
    let body: Value = resp.json().map_err(|_| json!({})).unwrap_or_default();
    let valid = body.get("valid").and_then(|v| v.as_bool()).unwrap_or(true);
    if !valid {
        mark_revoked(&conn);
        return Err("lisans iptal edilmiş".into());
    }
    clear_revoked(&conn);
    Ok(())
}

/// Arka planda periyodik online kontrol (6 saatte bir).
pub fn start_license_watch() {
    std::thread::spawn(|| loop {
        let _ = check_license_online();
        std::thread::sleep(Duration::from_secs(6 * 3600));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    #[test]
    fn token_roundtrip_verifies() {
        let sk = test_signing_key();
        let pk = sk.verifying_key();
        let payload = json!({
            "v": 1,
            "license_id": "L-1",
            "business_name": "Test Kafe",
            "machine_hash": "abc123",
            "issued_at": "2026-01-01T00:00:00Z",
            "expires_at": null
        });
        let token = encode_token(&payload, &sk);
        let parsed = parse_token_with(&token, &pk).unwrap();
        assert_eq!(parsed["license_id"], "L-1");
        assert_eq!(parsed["business_name"], "Test Kafe");
    }

    #[test]
    fn tampered_token_fails() {
        let sk = test_signing_key();
        let pk = sk.verifying_key();
        let payload = json!({ "v": 1, "license_id": "L-1", "machine_hash": "abc", "expires_at": null });
        let token = encode_token(&payload, &sk);
        let mut bytes = token.into_bytes();
        if let Some(pos) = bytes.iter().position(|b| *b == b'1') {
            bytes[pos] = b'2';
        }
        let tampered = String::from_utf8(bytes).unwrap();
        assert!(parse_token_with(&tampered, &pk).is_err());
    }

    #[test]
    fn wrong_key_fails() {
        let sk = test_signing_key();
        let other = SigningKey::from_bytes(&[9u8; 32]);
        let other_pk = other.verifying_key();
        let payload = json!({ "v": 1, "license_id": "L-1", "machine_hash": "abc", "expires_at": null });
        let token = encode_token(&payload, &sk);
        assert!(parse_token_with(&token, &other_pk).is_err());
    }

    #[test]
    fn perpetual_token_not_expired() {
        let payload = json!({ "v": 1, "expires_at": null });
        assert!(!is_expired(&payload));
    }
}
