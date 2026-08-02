use rusqlite::{params, Row};
use tauri::State;
use uuid::Uuid;
use chrono::Local;

use crate::db::AppState;
use crate::models::*;
use crate::commands::auth::log_audit_conn;
use crate::commands::settings::get_setting_value;

const DRINK_COLS: &str = "id, name, price, category, stock, emoji, description, cost, min_stock, is_active";

fn drink_from_row(row: &Row) -> rusqlite::Result<DrinkItem> {
    Ok(DrinkItem {
        id: row.get(0)?,
        name: row.get(1)?,
        price: row.get(2)?,
        category: row.get(3)?,
        stock: row.get(4)?,
        emoji: row.get(5)?,
        description: row.get(6)?,
        cost: row.get(7)?,
        min_stock: row.get(8)?,
        is_active: row.get(9)?,
    })
}

fn get_drink(conn: &rusqlite::Connection, drink_id: &str) -> Result<DrinkItem, String> {
    conn.query_row(&format!("SELECT {} FROM drinks WHERE id = ?1", DRINK_COLS), params![drink_id], |r| drink_from_row(r)).map_err(|_| "Ürün bulunamadı".into())
}

fn log_stock_movement(conn: &rusqlite::Connection, drink_id: &str, change: i64, reason: &str) -> Result<(), String> {
    let name: String = conn.query_row("SELECT name FROM drinks WHERE id = ?1", params![drink_id], |r| r.get(0)).map_err(|_| "Ürün bulunamadı")?;
    let after: i64 = conn.query_row("SELECT stock FROM drinks WHERE id = ?1", params![drink_id], |r| r.get(0)).unwrap_or(-1);
    conn.execute("INSERT INTO stock_movements (id, drink_id, drink_name, change_amount, stock_after, reason, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![Uuid::new_v4().to_string(), drink_id, name, change, after, reason, Local::now().to_rfc3339()]).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_drinks(state: State<AppState>) -> Result<Vec<DrinkItem>, String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let mut stmt = conn.prepare(&format!("SELECT {} FROM drinks ORDER BY is_active DESC, category, name", DRINK_COLS)).map_err(|e| e.to_string())?;
    let drinks = stmt.query_map([], |row| drink_from_row(row)).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).collect();
    Ok(drinks)
}

#[tauri::command]
pub fn add_drink(
    name: String,
    price: f64,
    category: String,
    emoji: String,
    stock: i64,
    state: State<AppState>,
    description: Option<String>,
    cost: Option<f64>,
    min_stock: Option<i64>,
    is_active: Option<i64>,
) -> Result<DrinkItem, String> {
    crate::commands::auth::require_admin(&state)?;
    let id = Uuid::new_v4().to_string();
    let description = description.unwrap_or_default();
    let cost = cost.unwrap_or(0.0);
    let min_stock = min_stock.unwrap_or(-1);
    let is_active = if is_active.unwrap_or(1) == 1 { 1 } else { 0 };
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    conn.execute(
        "INSERT INTO drinks (id, name, price, category, stock, emoji, description, cost, min_stock, is_active) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![id, name, price, category, stock, emoji, description, cost, min_stock, is_active],
    )
    .map_err(|e| format!("Ürün eklenemedi: {}", e))?;
    if stock >= 0 {
        let _ = log_stock_movement(&conn, &id, stock, "Başlangıç stoğu");
    }
    log_audit_conn(&conn, &state, "add_drink", "drinks", &name);
    Ok(DrinkItem { id, name, price, category, stock, emoji, description, cost, min_stock, is_active })
}

#[tauri::command]
pub fn update_drink(
    drink_id: String,
    name: String,
    price: f64,
    category: String,
    emoji: String,
    stock: i64,
    state: State<AppState>,
    description: Option<String>,
    cost: Option<f64>,
    min_stock: Option<i64>,
    is_active: Option<i64>,
) -> Result<(), String> {
    crate::commands::auth::require_admin(&state)?;
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let old = get_drink(&conn, &drink_id)?;
    let description = description.unwrap_or(old.description);
    let cost = cost.unwrap_or(old.cost);
    let min_stock = min_stock.unwrap_or(old.min_stock);
    let is_active = if is_active.unwrap_or(old.is_active) == 1 { 1 } else { 0 };
    conn.execute(
        "UPDATE drinks SET name=?1, price=?2, category=?3, emoji=?4, stock=?5, description=?6, cost=?7, min_stock=?8, is_active=?9 WHERE id=?10",
        params![name, price, category, emoji, stock, description, cost, min_stock, is_active, drink_id],
    )
    .map_err(|e| e.to_string())?;
    if old.stock != stock {
        let diff = stock - old.stock;
        if stock >= 0 {
            let reason = if old.stock == -1 { "Başlangıç stoğu".to_string() } else { format!("Stok düzeltme ({:+})", diff) };
            let _ = log_stock_movement(&conn, &drink_id, diff, &reason);
        }
    }
    log_audit_conn(&conn, &state, "update_drink", "drinks", &name);
    Ok(())
}

#[tauri::command]
pub fn set_drink_active(drink_id: String, is_active: i64, state: State<AppState>) -> Result<(), String> {
    crate::commands::auth::require_admin(&state)?;
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let active = if is_active == 1 { 1 } else { 0 };
    conn.execute("UPDATE drinks SET is_active = ?1 WHERE id = ?2", params![active, drink_id]).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "set_drink_active", "drinks", &drink_id);
    Ok(())
}

#[tauri::command]
pub fn remove_drink(drink_id: String, state: State<AppState>) -> Result<(), String> {
    crate::commands::auth::require_admin(&state)?;
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    conn.execute("DELETE FROM drinks WHERE id = ?1", params![drink_id]).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "remove_drink", "drinks", &drink_id);
    Ok(())
}

#[tauri::command]
pub fn order_drink(session_id: String, drink_id: String, quantity: i32, state: State<AppState>) -> Result<DrinkOrder, String> {
    if quantity < 1 { return Err("Geçerli bir adet girin".into()); }
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let (sname, cust) = conn.query_row("SELECT station_name, customer FROM active_sessions WHERE station_id = ?1", params![session_id], |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?))).map_err(|_| "Aktif oturum bulunamadı")?;
    let (dname, price, stock, is_active) = conn.query_row("SELECT name, price, stock, is_active FROM drinks WHERE id = ?1", params![drink_id], |r| Ok((r.get::<_,String>(0)?, r.get::<_,f64>(1)?, r.get::<_,i64>(2)?, r.get::<_,i64>(3)?))).map_err(|_| "İçecek bulunamadı")?;
    if is_active != 1 { return Err(format!("'{}' şu anda menüde değil", dname)); }
    if stock >= 0 && quantity as i64 > stock { return Err(format!("Stok yetersiz! Kalan: {}", stock)); }
    if stock >= 0 { conn.execute("UPDATE drinks SET stock = stock - ?1 WHERE id = ?2", params![quantity, drink_id]).map_err(|e| e.to_string())?; }
    let id = Uuid::new_v4().to_string();
    let total = price * quantity as f64;
    let ot = Local::now().to_rfc3339();
    conn.execute("INSERT INTO drink_orders (id,session_id,station_name,customer,drink_name,drink_id,price,quantity,total,order_time) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)", params![id,session_id,sname,cust,dname,drink_id,price,quantity,total,ot]).map_err(|e| e.to_string())?;
    if stock >= 0 {
        let _ = log_stock_movement(&conn, &drink_id, -(quantity as i64), &format!("Masaya eklendi: {} - {}", sname, cust));
    }
    log_audit_conn(&conn, &state, "add_drink_to_table", "drinks", format!("{} x{} -> {}", dname, quantity, sname).as_str());
    Ok(DrinkOrder { id, session_id, station_name: sname, customer: cust, drink_name: dname, price, quantity, total, order_time: ot })
}

#[tauri::command]
pub fn get_drink_orders(state: State<AppState>) -> Result<Vec<DrinkOrder>, String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let mut stmt = conn.prepare("SELECT id,session_id,station_name,customer,drink_name,price,quantity,total,order_time FROM drink_orders ORDER BY order_time DESC").map_err(|e| e.to_string())?;
    let orders = stmt.query_map([], |row| Ok(DrinkOrder { id: row.get(0)?, session_id: row.get(1)?, station_name: row.get(2)?, customer: row.get(3)?, drink_name: row.get(4)?, price: row.get(5)?, quantity: row.get(6)?, total: row.get(7)?, order_time: row.get(8)? })).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).collect();
    Ok(orders)
}

#[tauri::command]
pub fn remove_drink_order(order_id: String, state: State<AppState>) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let (drink_id, qty): (String, i64) = conn.query_row("SELECT drink_id, quantity FROM drink_orders WHERE id = ?1", params![order_id], |r| Ok((r.get(0)?, r.get(1)?))).map_err(|_| "Kayıt bulunamadı")?;
    conn.execute("DELETE FROM drink_orders WHERE id = ?1", params![order_id]).map_err(|e| e.to_string())?;
    let current_stock: i64 = conn.query_row("SELECT stock FROM drinks WHERE id = ?1", params![drink_id], |r| r.get(0)).unwrap_or(-1);
    if current_stock >= 0 {
        conn.execute("UPDATE drinks SET stock = stock + ?1 WHERE id = ?2", params![qty, drink_id]).map_err(|e| e.to_string())?;
        let _ = log_stock_movement(&conn, &drink_id, qty, "Masadan kaldırıldı");
    }
    log_audit_conn(&conn, &state, "remove_drink_from_table", "drinks", &order_id);
    Ok(())
}

#[tauri::command]
pub fn get_session_drink_items(session_id: String, state: State<AppState>) -> Result<Vec<DrinkOrder>, String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let mut stmt = conn.prepare("SELECT id,session_id,station_name,customer,drink_name,price,quantity,total,order_time FROM drink_orders WHERE session_id = ?1 ORDER BY order_time ASC").map_err(|e| e.to_string())?;
    let items = stmt.query_map(params![session_id], |row| Ok(DrinkOrder { id: row.get(0)?, session_id: row.get(1)?, station_name: row.get(2)?, customer: row.get(3)?, drink_name: row.get(4)?, price: row.get(5)?, quantity: row.get(6)?, total: row.get(7)?, order_time: row.get(8)? })).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).collect();
    Ok(items)
}

#[tauri::command]
pub fn adjust_stock(drink_id: String, change: i64, reason: String, state: State<AppState>) -> Result<DrinkItem, String> {
    crate::commands::auth::require_admin(&state)?;
    if change == 0 { return Err("Değişim 0 olamaz!".into()); }
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let (name, current): (String, i64) = conn.query_row("SELECT name, stock FROM drinks WHERE id = ?1", params![drink_id], |r| Ok((r.get(0)?, r.get(1)?))).map_err(|_| "Ürün bulunamadı")?;
    if current < 0 {
        return Err("Sınırsız (stok takipsiz) ürünlerde stok işlemi yapılamaz!".into());
    }
    let new_stock = current + change;
    if new_stock < 0 { return Err(format!("Stok negatif olamaz! Mevcut: {}", current)); }
    conn.execute("UPDATE drinks SET stock = ?1 WHERE id = ?2", params![new_stock, drink_id]).map_err(|e| e.to_string())?;
    let reason = if reason.trim().is_empty() { "Stok işlemi".to_string() } else { reason.trim().to_string() };
    let _ = log_stock_movement(&conn, &drink_id, change, &reason);
    log_audit_conn(&conn, &state, "adjust_stock", "drinks", format!("{} ({:+}) - {}", name, change, reason).as_str());
    get_drink(&conn, &drink_id)
}

#[tauri::command]
pub fn get_stock_movements(drink_id: String, state: State<AppState>) -> Result<Vec<StockMovement>, String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let mut stmt = conn.prepare("SELECT id, drink_id, drink_name, change_amount, stock_after, reason, created_at FROM stock_movements WHERE drink_id = ?1 ORDER BY created_at DESC LIMIT 100").map_err(|e| e.to_string())?;
    let moves = stmt.query_map(params![drink_id], |row| Ok(StockMovement {
        id: row.get(0)?, drink_id: row.get(1)?, drink_name: row.get(2)?,
        change_amount: row.get(3)?, stock_after: row.get(4)?, reason: row.get(5)?, created_at: row.get(6)?,
    })).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).collect();
    Ok(moves)
}

#[tauri::command]
pub fn get_low_stock_items(state: State<AppState>) -> Result<Vec<DrinkItem>, String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let threshold: i64 = get_setting_value(&conn, "low_stock_threshold").and_then(|v| v.parse().ok()).unwrap_or(5);
    let mut stmt = conn.prepare(&format!("SELECT {} FROM drinks WHERE stock >= 0 AND stock <= (CASE WHEN min_stock >= 0 THEN min_stock ELSE ?1 END) ORDER BY stock ASC", DRINK_COLS)).map_err(|e| e.to_string())?;
    let items = stmt.query_map(params![threshold], |row| drink_from_row(row)).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).collect();
    Ok(items)
}
