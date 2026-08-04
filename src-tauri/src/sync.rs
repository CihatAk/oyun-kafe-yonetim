use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use chrono::Local;
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::db::{get_data_dir, get_db_path};
use crate::queries;

const SYNC_SECONDS: u64 = 5;
const STALE_AFTER_SECONDS: i64 = 30;
const HISTORY_SYNC_DAYS: i64 = 30;

#[derive(Clone, serde::Serialize)]
pub struct SyncStatus {
    pub ok: bool,
    pub last_at: Option<String>,
    pub last_error: Option<String>,
}

static STATUS: Mutex<Option<SyncStatus>> = Mutex::new(None);

pub fn current_status() -> SyncStatus {
    STATUS
        .lock()
        .map(|g| g.clone())
        .ok()
        .flatten()
        .unwrap_or(SyncStatus { ok: false, last_at: None, last_error: Some("durum kilitlenemedi".into()) })
}

fn set_status(ok: bool, err: Option<String>) {
    if let Ok(mut g) = STATUS.lock() {
        *g = Some(SyncStatus {
            ok,
            last_at: Some(Local::now().to_rfc3339()),
            last_error: err,
        });
    }
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct SupabaseConfig {
    url: String,
    anon_key: String,
    service_key: String,
}

pub fn start() {
    thread::spawn(|| loop {
        let now = Local::now().to_rfc3339();
        match run_once() {
            Ok(()) => set_status(true, None),
            Err(e) => {
                eprintln!("[supabase] sync ({}): {}", now, e);
                set_status(false, Some(e));
            }
        }
        thread::sleep(Duration::from_secs(SYNC_SECONDS));
    });
}

fn config_path() -> PathBuf {
    get_data_dir().join("supabase.json")
}

fn load_config() -> Option<SupabaseConfig> {
    let raw = fs::read_to_string(config_path()).ok()?;
    serde_json::from_str(&raw).ok()
}

fn run_once() -> Result<(), String> {
    let cfg = load_config().ok_or("supabase.json bulunamadi")?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;

    let conn = Connection::open(get_db_path()).map_err(|e| e.to_string())?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
        .map_err(|e| e.to_string())?;
    let now = Local::now().to_rfc3339();

    // Her tablo ayrı ayrı eşitlenir; tek tabloda yaşanacak bir hata (ör. Supabase
    // şemasında eksik sütun) diğer tabloların eşitlenmesini engellemez.
    let mut errors: Vec<String> = Vec::new();
    let mut push_err = |res: Result<(), String>| {
        if let Err(e) = res {
            eprintln!("[supabase] {}: {}", Local::now().to_rfc3339(), e);
            errors.push(e);
        }
    };

    let ov = queries::overview(&conn);
    let summary = &ov["summary"];
    let overview_row = json!({
        "id": 1,
        "active_count": summary["active"],
        "idle_count": summary["idle"],
        "total_stations": summary["total"],
        "today_revenue": ov["today_revenue"],
        "today_drinks": ov["today_drinks"],
        "today_sessions": ov["today_sessions"],
        "live_estimate": ov["live_estimate"],
        "low_stock_threshold": ov["low_stock_threshold"],
        "updated_at": now,
    });
    push_err(upsert(&client, &cfg, "kafe_overview", &[overview_row]));

    let stations: Vec<Value> = ov["stations"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|s| {
            json!({
                "id": s["id"], "name": s["name"], "type": s["type"], "group_name": s["group"],
                "status": s["status"], "customer": s["customer"], "start_time": s["start_time"],
                "elapsed_min": s["elapsed_min"], "extra_controllers": s["extra_controllers"], "updated_at": now,
            })
        })
        .collect();
    push_err(upsert(&client, &cfg, "kafe_stations", &stations));

    let sessions: Vec<Value> = ov["sessions"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|s| {
            json!({
                "station_id": s["station_id"], "station_name": s["station_name"],
                "customer": s["customer"], "rate_type": s["rate_type"], "start_time": s["start_time"],
                "is_paused": s["is_paused"], "minutes": s["minutes"], "fee": s["fee"],
                "extra_controllers": s["extra_controllers"], "extra_fee": s["extra_fee"],
                "drink_total": s["drink_total"], "total": s["total"], "updated_at": now,
            })
        })
        .collect();
    let sessions_res = upsert(&client, &cfg, "kafe_sessions", &sessions);
    if sessions_res.is_ok() {
        push_err(delete_stale_sessions(&client, &cfg));
    }
    push_err(sessions_res);

    let drinks_val = queries::drinks(&conn);
    let drinks: Vec<Value> = drinks_val
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|d| {
            json!({
                "id": d["id"], "name": d["name"], "price": d["price"], "category": d["category"],
                "stock": d["stock"], "emoji": d["emoji"], "description": d["description"],
                "cost": d["cost"], "min_stock": d["min_stock"], "is_active": d["is_active"],
                "updated_at": now,
            })
        })
        .collect();
    push_err(upsert(&client, &cfg, "kafe_drinks", &drinks));

    let hist_val = queries::history_since_days(&conn, HISTORY_SYNC_DAYS);
    let hist: Vec<Value> = hist_val
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|h| {
            json!({
                "id": h["id"], "station_name": h["station_name"], "customer": h["customer"],
                "start_time": h["start_time"], "end_time": h["end_time"],
                "duration_minutes": h["duration_minutes"], "total": h["total"],
                "payment_method": h["payment_method"], "drink_total": h["drink_total"],
                "extra_controllers": h["extra_controllers"], "extra_fee": h["extra_fee"],
                "updated_at": now,
            })
        })
        .collect();
    push_err(upsert(&client, &cfg, "kafe_history", &hist));

    let today = Local::now().format("%Y-%m-%d").to_string();
    let de = queries::day_end(&conn, &today);
    push_err(upsert(&client, &cfg, "kafe_day_end", &[day_end_row(&today, &de, &now)]));

    let mut recent_dates: Vec<String> = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT DISTINCT date(end_time) FROM session_history WHERE date(end_time) >= date('now','-30 days') ORDER BY date(end_time)",
    ) {
        if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) {
            for d in rows.flatten() {
                recent_dates.push(d);
            }
        }
    }
    for d in recent_dates {
        if d == today {
            continue;
        }
        let de = queries::day_end(&conn, &d);
        push_err(upsert(&client, &cfg, "kafe_day_end", &[day_end_row(&d, &de, &now)]));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn day_end_row(date: &str, de: &Value, now: &str) -> Value {
    json!({
        "id": date,
        "sessions": de["sessions"],
        "total_revenue": de["total_revenue"],
        "total_discount": de["total_discount"],
        "drink_revenue": de["drink_revenue"],
        "avg_duration_minutes": de["avg_duration_minutes"],
        "cash_revenue": de["cash_revenue"],
        "card_revenue": de["card_revenue"],
        "other_revenue": de["other_revenue"],
        "partial_cash": de["partial_cash"],
        "partial_card": de["partial_card"],
        "top_drinks": de["top_drinks"],
        "top_stations": de["top_stations"],
        "updated_at": now,
    })
}

fn upsert(client: &reqwest::blocking::Client, cfg: &SupabaseConfig, table: &str, rows: &[Value]) -> Result<(), String> {
    if rows.is_empty() {
        return Ok(());
    }
    let url = format!("{}/rest/v1/{}", cfg.url, table);
    let resp = client
        .post(&url)
        .header("apikey", &cfg.service_key)
        .header("Authorization", format!("Bearer {}", cfg.service_key))
        .header("Content-Type", "application/json")
        .header("Prefer", "resolution=merge-duplicates,return=minimal")
        .json(rows)
        .send()
        .map_err(|e| format!("{}: {}", table, e))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("{}: {} {}", table, resp.status(), resp.text().unwrap_or_default()))
    }
}

fn delete_stale_sessions(client: &reqwest::blocking::Client, cfg: &SupabaseConfig) -> Result<(), String> {
    let cutoff = (Local::now() - chrono::Duration::seconds(STALE_AFTER_SECONDS))
        .to_utc()
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let url = format!("{}/rest/v1/kafe_sessions?updated_at=lt.{}", cfg.url, cutoff);
    let resp = client
        .delete(&url)
        .header("apikey", &cfg.service_key)
        .header("Authorization", format!("Bearer {}", cfg.service_key))
        .header("Prefer", "return=minimal")
        .send()
        .map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("stale: {} {}", resp.status(), resp.text().unwrap_or_default()))
    }
}
