use chrono::Local;
use rusqlite::{params, Connection};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use uuid::Uuid;

use crate::models::PricingConfig;

pub struct AppState {
    pub db: Mutex<Connection>,
    pub data_dir: PathBuf,
    pub current_user: Mutex<Option<CurrentUser>>,
}

#[derive(Clone, serde::Serialize)]
pub struct CurrentUser {
    pub id: String,
    pub username: String,
    pub full_name: String,
    pub role: String,
    pub permissions: String,
    pub must_change_password: bool,
}

impl CurrentUser {
    pub fn discount_limit(&self) -> f64 {
        if self.role == "admin" {
            return f64::MAX;
        }
        serde_json::from_str::<serde_json::Value>(&self.permissions)
            .ok()
            .and_then(|v| v.get("discount_limit").and_then(|x| x.as_f64()))
            .unwrap_or(0.0)
    }
}

pub fn get_data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("oyun-kafe-yonetim")
}

pub fn get_db_path() -> PathBuf {
    get_data_dir().join("database.sqlite")
}

pub fn migrate_db(conn: &Connection) {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS stations (
            id TEXT PRIMARY KEY, name TEXT NOT NULL, station_type TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'idle', group_name TEXT NOT NULL DEFAULT ''
        );
        CREATE TABLE IF NOT EXISTS active_sessions (
            station_id TEXT PRIMARY KEY, station_name TEXT NOT NULL, customer TEXT NOT NULL,
            start_time TEXT NOT NULL, rate_type TEXT NOT NULL, notes TEXT NOT NULL DEFAULT '',
            tags TEXT NOT NULL DEFAULT '', paused_at TEXT, total_paused_seconds INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS session_history (
            id TEXT PRIMARY KEY, station_name TEXT NOT NULL, customer TEXT NOT NULL,
            start_time TEXT NOT NULL, end_time TEXT NOT NULL, duration_minutes INTEGER NOT NULL,
            total REAL NOT NULL, payment_method TEXT NOT NULL, rate_type TEXT NOT NULL,
            drink_total REAL DEFAULT 0, notes TEXT NOT NULL DEFAULT '', tags TEXT NOT NULL DEFAULT ''
        );
        CREATE TABLE IF NOT EXISTS drinks (
            id TEXT PRIMARY KEY, name TEXT NOT NULL, price REAL NOT NULL,
            category TEXT NOT NULL DEFAULT 'icecek', stock INTEGER NOT NULL DEFAULT -1,
            emoji TEXT NOT NULL DEFAULT '', description TEXT NOT NULL DEFAULT '',
            cost REAL NOT NULL DEFAULT 0, min_stock INTEGER NOT NULL DEFAULT -1,
            is_active INTEGER NOT NULL DEFAULT 1
        );
        CREATE TABLE IF NOT EXISTS drink_orders (
            id TEXT PRIMARY KEY, session_id TEXT NOT NULL, station_name TEXT NOT NULL,
            customer TEXT NOT NULL, drink_name TEXT NOT NULL, price REAL NOT NULL,
            quantity INTEGER NOT NULL, total REAL NOT NULL, order_time TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS pricing_config (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS partial_payments (
            id TEXT PRIMARY KEY, session_id TEXT NOT NULL, payment_method TEXT NOT NULL,
            amount REAL NOT NULL, created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY, username TEXT NOT NULL UNIQUE, password_hash TEXT NOT NULL,
            full_name TEXT NOT NULL DEFAULT '', role TEXT NOT NULL DEFAULT 'calisan',
            active INTEGER NOT NULL DEFAULT 1, created_at TEXT NOT NULL,
            permissions TEXT NOT NULL DEFAULT '{}'
        );
        CREATE TABLE IF NOT EXISTS audit_log (
            id TEXT PRIMARY KEY, user_id TEXT NOT NULL DEFAULT '', user_name TEXT NOT NULL DEFAULT '',
            action TEXT NOT NULL, entity TEXT NOT NULL DEFAULT '', detail TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS stock_movements (
            id TEXT PRIMARY KEY, drink_id TEXT NOT NULL, drink_name TEXT NOT NULL,
            change_amount INTEGER NOT NULL, stock_after INTEGER NOT NULL,
            reason TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY, value TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_hist_end ON session_history(end_time);
        CREATE INDEX IF NOT EXISTS idx_hist_station ON session_history(station_name);
        CREATE INDEX IF NOT EXISTS idx_hist_pay ON session_history(payment_method);
        CREATE INDEX IF NOT EXISTS idx_dorders_sess ON drink_orders(session_id);
        CREATE INDEX IF NOT EXISTS idx_audit_created ON audit_log(created_at);
        CREATE INDEX IF NOT EXISTS idx_stockmov_drink ON stock_movements(drink_id);
        ",
    )
    .unwrap();

    let alter_cols: &[(&str, &str)] = &[
        ("active_sessions", "notes TEXT NOT NULL DEFAULT ''"),
        ("active_sessions", "tags TEXT NOT NULL DEFAULT ''"),
        ("active_sessions", "paused_at TEXT"),
        ("active_sessions", "total_paused_seconds INTEGER NOT NULL DEFAULT 0"),
        ("active_sessions", "extra_controllers INTEGER NOT NULL DEFAULT 0"),
        ("stations", "group_name TEXT NOT NULL DEFAULT ''"),
        ("drinks", "category TEXT NOT NULL DEFAULT 'icecek'"),
        ("drinks", "stock INTEGER NOT NULL DEFAULT -1"),
        ("drinks", "emoji TEXT NOT NULL DEFAULT ''"),
        ("drinks", "description TEXT NOT NULL DEFAULT ''"),
        ("drinks", "cost REAL NOT NULL DEFAULT 0"),
        ("drinks", "min_stock INTEGER NOT NULL DEFAULT -1"),
        ("drinks", "is_active INTEGER NOT NULL DEFAULT 1"),
        ("session_history", "drink_total REAL DEFAULT 0"),
        ("session_history", "notes TEXT NOT NULL DEFAULT ''"),
        ("session_history", "tags TEXT NOT NULL DEFAULT ''"),
        ("session_history", "extra_controllers INTEGER NOT NULL DEFAULT 0"),
        ("session_history", "extra_fee REAL NOT NULL DEFAULT 0"),
        ("drink_orders", "drink_id TEXT NOT NULL DEFAULT ''"),
        ("session_history", "discount REAL DEFAULT 0"),
        ("session_history", "discount_reason TEXT NOT NULL DEFAULT ''"),
        ("users", "permissions TEXT NOT NULL DEFAULT '{}'"),
        ("users", "must_change_password INTEGER NOT NULL DEFAULT 0"),
    ];
    for (table, col_def) in alter_cols {
        let col_name = col_def.split_whitespace().next().unwrap();
        let check: bool = conn
            .prepare(&format!("PRAGMA table_info({})", table))
            .ok()
            .and_then(|mut stmt| {
                stmt.query_map([], |row| row.get::<_, String>(1))
                    .ok()
                    .map(|cols| cols.filter_map(|c| c.ok()).any(|c| c == col_name))
            })
            .unwrap_or(false);
        if !check {
            let _ = conn.execute_batch(&format!("ALTER TABLE {} ADD COLUMN {};", table, col_def));
        }
    }

    migrate_versioned(conn);
}

const SCHEMA_VERSION: i64 = 3;

fn migrate_versioned(conn: &Connection) {
    let current: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap_or(0);
    if current < 1 {
        // Sürüm 1: kullanıcı şifreleri Argon2 + kişisel tuz kullanımına geçti
        // (şema değişikliği gerekmez; PHC dizesi tuzu içinde barındırır)
        let _ = conn.execute_batch("PRAGMA user_version = 1");
    }
    if current < 2 {
        // Sürüm 2: kampanya/paket/promo sistemi tamamen kaldırıldı.
        // Manuel indirim (personel için sınırlı) bu sistemin yerini aldı.
        for tbl in ["campaigns", "packages", "promo_codes"] {
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    params![tbl],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0)
                > 0;
            if exists {
                let _ = conn.execute_batch(&format!("DROP TABLE {};", tbl));
            }
        }
        let _ = conn.execute_batch("PRAGMA user_version = 2");
    }
    if current < 3 {
        // Sürüm 3: VIP sistemi ve vardiya özelliği tamamen kaldırıldı.
        // - VIP istasyonları 'standard' tipe dönüştürülür.
        // - VIP tarifesi ayar kaydı silinir.
        // - Vardiya tablosu (varsa) kaldırılır.
        let _ = conn.execute_batch("UPDATE stations SET station_type = 'standard' WHERE station_type = 'vip';");
        let _ = conn.execute_batch("DELETE FROM pricing_config WHERE key = 'vip_per_minute';");
        let shifts_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'shifts'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0;
        if shifts_exists {
            let _ = conn.execute_batch("DROP TABLE shifts;");
        }
        let _ = conn.execute_batch(&format!("PRAGMA user_version = {}", SCHEMA_VERSION));
    }
}

pub fn seed_defaults(conn: &Connection) {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM stations", [], |r| r.get(0)).unwrap_or(0);
    if count == 0 {
        for (id, name, stype) in [("pc-01","PC-01","standard"),("pc-02","PC-02","standard"),("pc-03","PC-03","standard"),("pc-04","PC-04","standard"),("pc-05","PC-05","standard"),("pc-06","PC-06","standard")] {
            conn.execute("INSERT INTO stations (id, name, station_type, status) VALUES (?1, ?2, ?3, 'idle')", params![id, name, stype]).unwrap();
        }
    }
    let dc: i64 = conn.query_row("SELECT COUNT(*) FROM drinks", [], |r| r.get(0)).unwrap_or(0);
    if dc == 0 {
        for (id, name, price, emoji) in [("su","Su",30.0,"💧"),("soda","Soda",30.0,"🥤"),("kutu","Kutu İçecek",80.0,"🥤")] {
            conn.execute("INSERT INTO drinks (id, name, price, category, stock, emoji) VALUES (?1, ?2, ?3, 'icecek', -1, ?5)", params![id, name, price, "", emoji]).unwrap();
        }
    }
    let uc: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0)).unwrap_or(0);
    if uc == 0 {
        let hash = crate::commands::auth::make_hash("admin123");
        conn.execute("INSERT INTO users (id, username, password_hash, full_name, role, active, must_change_password, created_at) VALUES (?1, 'admin', ?2, 'Yönetici', 'admin', 1, 1, ?3)",
            params![Uuid::new_v4().to_string(), hash, Local::now().to_rfc3339()]).unwrap();
    }
}

impl AppState {
    pub fn new() -> Self {
        let data_dir = get_data_dir();
        fs::create_dir_all(&data_dir).ok();
        let conn = Connection::open(get_db_path()).expect("Veritabanı açılamadı");
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL;").unwrap();
        migrate_db(&conn);
        seed_defaults(&conn);
        AppState { db: Mutex::new(conn), data_dir, current_user: Mutex::new(None) }
    }

    pub fn backup_db(&self) {
        let backup_dir = self.data_dir.join("backups");
        fs::create_dir_all(&backup_dir).ok();
        let filename = format!("backup_{}.sqlite", Local::now().format("%Y%m%d_%H%M%S"));
        let backup_path = backup_dir.join(&filename);
        let bs = backup_path.to_string_lossy().to_string();
        { let db = self.db.lock().unwrap(); let _ = db.execute_batch(&format!("VACUUM INTO '{}';", bs)); }
        let base = get_db_path().to_str().unwrap().to_string();
        let _ = fs::copy(base.clone() + "-wal", bs.clone() + "-wal");
        let _ = fs::copy(base + "-shm", bs + "-shm");
        self.cleanup_old_backups(&backup_dir);
    }

    fn cleanup_old_backups(&self, dir: &PathBuf) {
        if let Ok(entries) = fs::read_dir(dir) {
            let mut b: Vec<_> = entries.filter_map(|e| e.ok()).filter(|e| e.path().extension().is_some_and(|x| x == "sqlite")).collect();
            b.sort_by_key(|e| std::cmp::Reverse(e.file_name()));
            for e in b.into_iter().skip(30) { let _ = fs::remove_file(e.path()); }
        }
    }

    pub fn load_pricing(&self) -> PricingConfig {
        let conn = self.db.lock().unwrap();
        Self::load_pricing_conn(&conn)
    }

    pub fn load_pricing_conn(conn: &Connection) -> PricingConfig {
        let g = |k: &str| -> Option<String> { conn.query_row("SELECT value FROM pricing_config WHERE key = ?1", params![k], |r| r.get(0)).ok() };
        PricingConfig {
            cash_per_minute: g("cash_per_minute").and_then(|v| v.parse().ok()).unwrap_or(4.20),
            card_per_minute: g("card_per_minute").and_then(|v| v.parse().ok()).unwrap_or(5.00),
            min_charge: g("min_charge").and_then(|v| v.parse().ok()).unwrap_or(0.0),
            round_minutes: g("round_minutes").and_then(|v| v.parse().ok()).unwrap_or(1),
            extra_controller_per_hour: g("extra_controller_per_hour").and_then(|v| v.parse().ok()).unwrap_or(75.00),
            max_session_minutes: g("max_session_minutes").and_then(|v| v.parse().ok()).unwrap_or(0),
            warning_before_minutes: g("warning_before_minutes").and_then(|v| v.parse().ok()).unwrap_or(5),
        }
    }

    pub fn get_effective_rate(&self, station_type: &str, rate_type: &str, pricing: &PricingConfig) -> f64 {
        let _ = station_type;
        if rate_type == "nakit" { pricing.cash_per_minute }
        else { pricing.card_per_minute }
    }
}
