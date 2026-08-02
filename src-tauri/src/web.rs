use std::collections::HashMap;
use std::io::Cursor;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::Local;
use rusqlite::{params, Connection};
use serde_json::json;
use tiny_http::{Header, Method, Response, Server, StatusCode};
use uuid::Uuid;

use crate::commands::auth::verify_password;
use crate::sync;

pub const WEB_PORT: u16 = 8747;
const TOKEN_TTL: Duration = Duration::from_secs(4 * 3600);
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

fn web_host() -> String {
    std::env::var("OYUNKAFE_WEB_HOST").unwrap_or_else(|_| "127.0.0.1".into())
}

fn lan_enabled() -> bool {
    web_host() == "0.0.0.0"
}

pub fn run(db_path: PathBuf, port: u16) {
    let host = web_host();
    let addr = format!("{}:{}", host, port);
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

fn rendered_index() -> String {
    let mut out = INDEX.to_string();
    if let Some(raw) = std::fs::read_to_string(crate::db::get_data_dir().join("supabase.json")).ok() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
            let url = v["url"].as_str().unwrap_or("");
            let key = v["anon_key"].as_str().unwrap_or("");
            out = out.replace("__SUPABASE_URL__", url).replace("__SUPABASE_ANON_KEY__", key);
        }
    }
    out
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
        html_resp(200, &rendered_index())
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
    if !verify_password(&password, &hash) {
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

fn overview_handler(conn: &Connection, _user: &str) -> HttpResp {
    json_resp(200, crate::queries::overview(conn))
}

fn drinks_handler(conn: &Connection, _user: &str) -> HttpResp {
    json_resp(200, crate::queries::drinks(conn))
}

fn history_handler(conn: &Connection, _user: &str, limit: i64) -> HttpResp {
    json_resp(200, crate::queries::history(conn, limit))
}


#[tauri::command]
pub fn get_sync_status() -> Result<serde_json::Value, String> {
    Ok(serde_json::to_value(sync::current_status()).unwrap_or_else(|_| json!({ "ok": false })))
}

#[tauri::command]
pub fn get_web_info() -> Result<serde_json::Value, String> {
    let port = port_from_env();
    let ip = local_ip_address::local_ip().ok();
    let ip_str = ip.map(|i| i.to_string()).unwrap_or_default();
    let lan_url = if lan_enabled() && !ip_str.is_empty() {
        format!("http://{}:{}", ip_str, port)
    } else {
        String::new()
    };
    let sb = std::fs::read_to_string(crate::db::get_data_dir().join("supabase.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok());
    let has_sb = sb.as_ref().and_then(|v| v["url"].as_str().map(String::from)).is_some();
    let panel_url = sb
        .as_ref()
        .and_then(|v| v["panel_url"].as_str().map(String::from))
        .filter(|u| !u.is_empty())
        .or_else(|| if has_sb { Some("https://panel-deploy-six.vercel.app".to_string()) } else { None })
        .unwrap_or_default();
    let lan_note = if lan_enabled() {
        String::new()
    } else {
        "LAN erişimi kapalı. Açmak için OYUNKAFE_WEB_HOST=0.0.0.0 ortam değişkeni ile başlatın.".to_string()
    };
    Ok(json!({
        "port": port,
        "ip": ip_str,
        "localUrl": format!("http://127.0.0.1:{}", port),
        "lanUrl": lan_url,
        "lanNote": lan_note,
        "panelUrl": panel_url,
        "externalNote": "İnternet paneli: tarayıcıda 'Panel adresi'ni açın (Supabase üzerinden, giriş gerektirmez)."
    }))
}
