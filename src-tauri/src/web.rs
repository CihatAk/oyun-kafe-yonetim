use std::collections::HashMap;
use std::io::Cursor;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local};
use rusqlite::{params, Connection};
use serde_json::json;
use tiny_http::{Header, Method, Response, Server, StatusCode};
use uuid::Uuid;

use crate::commands::auth::hash_password;
use crate::db::AppState;

pub const WEB_PORT: u16 = 8747;
const SALT: &str = "oyun-kafe-2026";
const TOKEN_TTL: Duration = Duration::from_secs(12 * 3600);
const INDEX: &str = include_str!("../web/index.html");

struct TokenInfo {
    username: String,
    expires: Instant,
}

static TOKENS: Mutex<Option<HashMap<String, TokenInfo>>> = Mutex::new(None);

fn port_from_env() -> u16 {
    std::env::var("OYUNKAFE_WEB_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(WEB_PORT)
}

pub fn run(db_path: PathBuf, port: u16) {
    let addr = format!("0.0.0.0:{}", port);
    let server = match Server::http(&addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Web görüntüleme sunucusu başlatılamadı ({}): {}", addr, e);
            return;
        }
    };
    eprintln!("Web görüntüleme sunucusu hazır: http://127.0.0.1:{}", port);
    for request in server.incoming_requests() {
        handle_request(&db_path, request);
    }
}

fn connect(db_path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    conn.execute_batch("PRAGMA query_only=ON; PRAGMA busy_timeout=5000;")
        .map_err(|e| e.to_string())?;
    Ok(conn)
}

type HttpResp = Response<Cursor<Vec<u8>>>;

fn json_resp(code: u16, value: serde_json::Value) -> HttpResp {
    let ct = Header::from_bytes("Content-Type", "application/json; charset=utf-8").unwrap();
    Response::from_string(value.to_string())
        .with_status_code(StatusCode(code))
        .with_header(ct)
}

fn empty_resp(code: u16) -> HttpResp {
    Response::from_string(String::new()).with_status_code(StatusCode(code))
}

fn html_resp(code: u16, body: &str) -> HttpResp {
    let ct = Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap();
    Response::from_string(body.to_string())
        .with_status_code(StatusCode(code))
        .with_header(ct)
}

fn add_cors(resp: HttpResp) -> HttpResp {
    let allow_origin = Header::from_bytes("Access-Control-Allow-Origin", "*").unwrap();
    let allow_headers = Header::from_bytes(
        "Access-Control-Allow-Headers",
        "Content-Type, Authorization",
    )
    .unwrap();
    let allow_methods = Header::from_bytes("Access-Control-Allow-Methods", "GET, POST, OPTIONS").unwrap();
    resp.with_header(allow_origin)
        .with_header(allow_headers)
        .with_header(allow_methods)
}

fn issue_token(username: &str) -> String {
    let t = Uuid::new_v4().to_string();
    let mut guard = TOKENS.lock().unwrap_or_else(|p| p.into_inner());
    guard
        .get_or_insert_with(HashMap::new)
        .insert(t.clone(), TokenInfo {
            username: username.to_string(),
            expires: Instant::now() + TOKEN_TTL,
        });
    t
}

fn check_token(tok: &str) -> Option<String> {
    let mut guard = TOKENS.lock().unwrap_or_else(|p| p.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    map.retain(|_, v| v.expires > Instant::now());
    map.get(tok).map(|v| v.username.clone())
}

fn drop_token(tok: &str) {
    let mut guard = TOKENS.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(map) = guard.as_mut() {
        map.remove(tok);
    }
}

fn handle_request(db_path: &Path, mut request: tiny_http::Request) {
    let method = request.method().clone();
    let url = request.url().to_string();
    let auth = request
        .headers()
        .iter()
        .find(|h| h.field.as_str().to_ascii_lowercase() == "authorization")
        .and_then(|h| {
            let v = h.value.as_str();
            v.strip_prefix("Bearer ").map(|s| s.trim().to_string())
        });

    let mut body = String::new();
    if method == Method::Post {
        let _ = request
            .as_reader()
            .take(1024 * 1024)
            .read_to_string(&mut body);
    }

    let resp: HttpResp = if url == "/" || url == "/index.html" || url.starts_with("/web") {
        html_resp(200, INDEX)
    } else if url.starts_with("/api/") {
        route_api(db_path, &method, &url, &body, auth.as_deref())
    } else {
        json_resp(404, json!({ "error": "Bulunamadı" }))
    };

    let _ = request.respond(add_cors(resp));
}

fn need_auth<F>(db_path: &Path, auth: Option<&str>, f: F) -> HttpResp
where
    F: Fn(&Connection, &str) -> HttpResp,
{
    match auth.and_then(check_token) {
        Some(username) => match connect(db_path) {
            Ok(conn) => f(&conn, &username),
            Err(e) => json_resp(500, json!({ "error": e })),
        },
        None => json_resp(401, json!({ "error": "Yetkisiz erişim" })),
    }
}

fn route_api(
    db_path: &Path,
    method: &Method,
    url: &str,
    body: &str,
    auth: Option<&str>,
) -> HttpResp {
    let path = url.split('?').next().unwrap_or("").to_string();
    let query = url.split('?').nth(1).unwrap_or("");

    if method == &Method::Options {
        return add_cors(empty_resp(204));
    }

    match (method, path.as_str()) {
        (&Method::Get, "/api/ping") => json_resp(200, json!({ "ok": true, "time": Local::now().to_rfc3339() })),
        (&Method::Post, "/api/login") => login_handler(db_path, body),
        (&Method::Post, "/api/logout") => {
            if let Some(t) = auth {
                drop_token(t);
            }
            json_resp(200, json!({ "ok": true }))
        }
        (&Method::Get, "/api/overview") => need_auth(db_path, auth, overview_handler),
        (&Method::Get, "/api/drinks") => need_auth(db_path, auth, drinks_handler),
        (&Method::Get, "/api/history") => {
            let lim: i64 = query
                .split('&')
                .find_map(|p| p.strip_prefix("limit="))
                .and_then(|v| v.parse().ok())
                .unwrap_or(30);
            need_auth(db_path, auth, move |conn, user| history_handler(conn, user, lim))
        }
        _ => json_resp(404, json!({ "error": "Bulunamadı" })),
    }
}

fn login_handler(db_path: &Path, body: &str) -> HttpResp {
    let data: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    let username = data["username"].as_str().unwrap_or("").trim().to_string();
    let password = data["password"].as_str().unwrap_or("").to_string();
    if username.is_empty() || password.is_empty() {
        return json_resp(400, json!({ "error": "Kullanıcı adı ve şifre gerekli" }));
    }
    let conn = match connect(db_path) {
        Ok(c) => c,
        Err(e) => return json_resp(500, json!({ "error": e })),
    };
    let user = conn
        .query_row(
            "SELECT id, username, password_hash, full_name, role, active FROM users WHERE username = ?1",
            params![username],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, i64>(5)?)),
        );
    let (id, uname, hash, full_name, role, active) = match user {
        Ok(u) => u,
        Err(_) => return json_resp(401, json!({ "error": "Kullanıcı bulunamadı" })),
    };
    if active != 1 {
        return json_resp(403, json!({ "error": "Kullanıcı pasif" }));
    }
    if hash != hash_password(&password, SALT) {
        return json_resp(401, json!({ "error": "Yanlış şifre" }));
    }
    let token = issue_token(&uname);
    json_resp(
        200,
        json!({
            "token": token,
            "user": { "id": id, "username": uname, "full_name": full_name, "role": role }
        }),
    )
}

fn scalar_i64(conn: &Connection, sql: &str, p: impl rusqlite::Params) -> i64 {
    conn.query_row(sql, p, |r| r.get::<_, i64>(0)).unwrap_or(0)
}

fn scalar_f64(conn: &Connection, sql: &str, p: impl rusqlite::Params) -> f64 {
    conn.query_row(sql, p, |r| r.get::<_, f64>(0)).unwrap_or(0.0)
}

fn session_fee(
    conn: &Connection,
    station_id: &str,
    rate_type: &str,
    start_str: &str,
    paused_at: Option<&str>,
    total_paused: i64,
) -> (i64, i64, f64, bool) {
    let st_type: String = conn
        .query_row("SELECT COALESCE(station_type,'standard') FROM stations WHERE id = ?1", params![station_id], |r| r.get(0))
        .unwrap_or_else(|_| "standard".into());
    let start = DateTime::parse_from_rfc3339(start_str)
        .map(|d| d.with_timezone(&Local))
        .unwrap_or_else(|_| Local::now());
    let now = Local::now();
    let total_secs = now.signed_duration_since(start).num_seconds().max(0);
    let is_paused = paused_at.is_some();
    let mut eff_secs = total_secs;
    if let Some(p) = paused_at {
        if let Ok(pd) = DateTime::parse_from_rfc3339(p) {
            let ps = now.signed_duration_since(pd.with_timezone(&Local)).num_seconds().max(0);
            eff_secs = eff_secs.saturating_sub(ps);
        }
    }
    eff_secs = eff_secs.saturating_sub(total_paused).max(0);
    let mins = eff_secs / 60;
    let secs = eff_secs % 60;
    let pricing = AppState::load_pricing_conn(conn);
    let per_min = if st_type == "vip" {
        pricing.vip_per_minute
    } else if rate_type == "nakit" {
        pricing.cash_per_minute
    } else {
        pricing.card_per_minute
    };
    let round_mins = pricing.round_minutes.max(1);
    let chunks = ((mins as f64) / (round_mins as f64)).ceil() as i64;
    let rounded = chunks * round_mins;
    let fee = (rounded as f64 * per_min).max(pricing.min_charge);
    (mins, secs, fee, is_paused)
}

fn overview_handler(conn: &Connection, _user: &str) -> HttpResp {
    let today = Local::now().date_naive().to_string();

    let active = scalar_i64(conn, "SELECT COUNT(*) FROM active_sessions", []);
    let idle = scalar_i64(conn, "SELECT COUNT(*) FROM stations WHERE status = 'idle'", []);
    let total = scalar_i64(conn, "SELECT COUNT(*) FROM stations", []);
    let vip_total = scalar_i64(conn, "SELECT COUNT(*) FROM stations WHERE station_type = 'vip'", []);
    let vip_busy = scalar_i64(
        conn,
        "SELECT COUNT(*) FROM stations s JOIN active_sessions a ON a.station_id = s.id WHERE s.station_type = 'vip'",
        [],
    );
    let today_rev = scalar_f64(conn, "SELECT COALESCE(SUM(total),0) FROM session_history WHERE date(end_time) = ?1", params![today]);
    let today_drinks = scalar_f64(conn, "SELECT COALESCE(SUM(total),0) FROM drink_orders WHERE date(order_time) = ?1", params![today]);
    let today_sessions = scalar_i64(conn, "SELECT COUNT(*) FROM session_history WHERE date(end_time) = ?1", params![today]);
    let campaigns_active = scalar_i64(conn, "SELECT COUNT(*) FROM campaigns WHERE active = 1", []);
    let packages_active = scalar_i64(conn, "SELECT COUNT(*) FROM packages WHERE active = 1", []);
    let low_threshold: i64 = crate::commands::settings::get_setting_value(conn, "low_stock_threshold")
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);

    let mut stations: Vec<serde_json::Value> = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT s.id, s.name, s.station_type, s.group_name, s.status, COALESCE(a.customer,''), a.start_time, a.paused_at, COALESCE(a.total_paused_seconds,0) FROM stations s LEFT JOIN active_sessions a ON a.station_id = s.id ORDER BY s.group_name, s.name",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, i64>(8)?,
            ))
        }) {
            for r in rows.flatten() {
                let (id, name, stype, group, status, customer, start, paused_at, total_paused) = r;
                let elapsed_min = if status == "active" {
                    let (_m, _s, _f, _p) = session_fee(conn, &id, "card", start.as_deref().unwrap_or(""), paused_at.as_deref(), total_paused);
                    _m
                } else {
                    0
                };
                stations.push(json!({
                    "id": id, "name": name, "type": stype, "group": group,
                    "status": status, "customer": customer, "start_time": start,
                    "elapsed_min": elapsed_min,
                }));
            }
        }
    }

    let mut sessions: Vec<serde_json::Value> = Vec::new();
    let mut live_estimate = 0.0f64;
    if let Ok(mut stmt) = conn.prepare(
        "SELECT station_id, station_name, customer, start_time, rate_type, paused_at, COALESCE(total_paused_seconds,0) FROM active_sessions",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, i64>(6)?,
            ))
        }) {
            for r in rows.flatten() {
                let (station_id, station_name, customer, start_time, rate_type, paused_at, total_paused) = r;
                let (mins, _secs, fee, is_paused) =
                    session_fee(conn, &station_id, &rate_type, &start_time, paused_at.as_deref(), total_paused);
                live_estimate += fee;
                let drink_total =
                    scalar_f64(conn, "SELECT COALESCE(SUM(total),0) FROM drink_orders WHERE session_id = ?1", params![station_id]);
                sessions.push(json!({
                    "station_id": station_id, "station_name": station_name,
                    "customer": customer, "rate_type": rate_type, "start_time": start_time,
                    "is_paused": is_paused, "minutes": mins, "fee": fee,
                    "drink_total": drink_total, "total": fee + drink_total,
                }));
            }
        }
    }

    let mut low_stock: Vec<serde_json::Value> = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT id, name, price, category, stock, emoji, min_stock FROM drinks WHERE stock >= 0 AND stock <= (CASE WHEN min_stock >= 0 THEN min_stock ELSE ?1 END) ORDER BY stock ASC",
    ) {
        if let Ok(rows) = stmt.query_map(params![low_threshold], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, f64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)?,
            ))
        }) {
            for r in rows.flatten() {
                let (id, name, price, category, stock, emoji, min_stock) = r;
                low_stock.push(json!({ "id": id, "name": name, "price": price, "category": category, "stock": stock, "emoji": emoji, "min_stock": min_stock }));
            }
        }
    }

    json_resp(
        200,
        json!({
            "server_time": Local::now().to_rfc3339(),
            "today": today,
            "summary": { "active": active, "idle": idle, "total": total, "vip_total": vip_total, "busy_vip": vip_busy },
            "today_revenue": today_rev,
            "today_drinks": today_drinks,
            "today_sessions": today_sessions,
            "live_estimate": live_estimate,
            "low_stock_threshold": low_threshold,
            "low_stock": low_stock,
            "campaigns_active": campaigns_active,
            "packages_active": packages_active,
            "stations": stations,
            "sessions": sessions,
        }),
    )
}

fn drinks_handler(conn: &Connection, _user: &str) -> HttpResp {
    let mut items: Vec<serde_json::Value> = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT id, name, price, category, stock, emoji, COALESCE(description,''), cost, min_stock, is_active FROM drinks ORDER BY is_active DESC, category, name",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, f64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, f64>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, i64>(9)?,
            ))
        }) {
            for r in rows.flatten() {
                let (id, name, price, category, stock, emoji, description, cost, min_stock, is_active) = r;
                items.push(json!({
                    "id": id, "name": name, "price": price, "category": category,
                    "stock": stock, "emoji": emoji, "description": description,
                    "cost": cost, "min_stock": min_stock, "is_active": is_active,
                }));
            }
        }
    }
    json_resp(200, json!(items))
}

fn history_handler(conn: &Connection, _user: &str, limit: i64) -> HttpResp {
    let mut items: Vec<serde_json::Value> = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT station_name, customer, start_time, end_time, duration_minutes, total, payment_method, COALESCE(drink_total,0) FROM session_history ORDER BY end_time DESC LIMIT ?1",
    ) {
        if let Ok(rows) = stmt.query_map(params![limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, f64>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, f64>(7)?,
            ))
        }) {
            for r in rows.flatten() {
                let (station_name, customer, start_time, end_time, duration_minutes, total, payment_method, drink_total) = r;
                items.push(json!({
                    "station_name": station_name, "customer": customer,
                    "start_time": start_time, "end_time": end_time,
                    "duration_minutes": duration_minutes, "total": total,
                    "payment_method": payment_method, "drink_total": drink_total,
                }));
            }
        }
    }
    json_resp(200, json!(items))
}

#[tauri::command]
pub fn get_web_info() -> Result<serde_json::Value, String> {
    let port = port_from_env();
    let ip = local_ip_address::local_ip().ok();
    let ip_str = ip.map(|i| i.to_string()).unwrap_or_default();
    let lan_url = if ip_str.is_empty() {
        String::new()
    } else {
        format!("http://{}:{}", ip_str, port)
    };
    Ok(json!({
        "port": port,
        "ip": ip_str,
        "localUrl": format!("http://127.0.0.1:{}", port),
        "lanUrl": lan_url,
        "externalNote": "İnternetten erişim için bir tünel (örn. Cloudflare Tunnel) veya port yönlendirme gerekir."
    }))
}
