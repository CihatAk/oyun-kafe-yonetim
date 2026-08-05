use chrono::Local;
use rusqlite::params;
use std::fs;
use tauri::State;

use crate::db::AppState;
use crate::models::*;
use crate::commands::auth::log_audit_conn;

#[tauri::command]
pub fn get_pricing(state: State<AppState>) -> Result<PricingConfig, String> {
    crate::commands::auth::require_admin(&state)?;
    Ok(state.load_pricing())
}

#[tauri::command]
pub fn set_pricing(config: PricingConfig, state: State<AppState>) -> Result<(), String> {
    crate::commands::auth::require_admin(&state)?;
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    for (k, v) in [("cash_per_minute",config.cash_per_minute),("card_per_minute",config.card_per_minute),("min_charge",config.min_charge),("extra_controller_per_hour",config.extra_controller_per_hour)] {
        conn.execute("INSERT OR REPLACE INTO pricing_config (key, value) VALUES (?1, ?2)", params![k, v.to_string()]).map_err(|e| e.to_string())?;
    }
    for (k, v) in [("round_minutes",config.round_minutes.max(1)),("max_session_minutes",config.max_session_minutes),("warning_before_minutes",config.warning_before_minutes)] {
        conn.execute("INSERT OR REPLACE INTO pricing_config (key, value) VALUES (?1, ?2)", params![k, v.to_string()]).map_err(|e| e.to_string())?;
    }
    log_audit_conn(&conn, &state, "set_pricing", "pricing", "Ücret ayarları güncellendi");
    Ok(())
}

fn map_history(row: &rusqlite::Row) -> rusqlite::Result<SessionRecord> {
    Ok(SessionRecord {
        id: row.get(0)?, station_name: row.get(1)?, customer: row.get(2)?,
        start_time: row.get(3)?, end_time: row.get(4)?, duration_minutes: row.get(5)?,
        total: row.get(6)?, payment_method: row.get(7)?, rate_type: row.get(8)?,
        drink_total: row.get(9)?, discount: row.get(10)?, notes: row.get(11)?, tags: row.get(12)?,
        extra_controllers: row.get(13)?, extra_fee: row.get(14)?,
    })
}

#[tauri::command]
pub fn get_history(state: State<AppState>) -> Result<Vec<SessionRecord>, String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let mut stmt = conn.prepare("SELECT id,station_name,customer,start_time,end_time,duration_minutes,total,payment_method,rate_type,COALESCE(drink_total,0),COALESCE(discount,0),COALESCE(notes,''),COALESCE(tags,''),COALESCE(extra_controllers,0),COALESCE(extra_fee,0) FROM session_history ORDER BY end_time DESC LIMIT 500").map_err(|e| e.to_string())?;
    let records = stmt.query_map([], map_history).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).collect();
    Ok(records)
}

#[tauri::command]
pub fn get_history_filtered(filter: HistoryFilter, state: State<AppState>) -> Result<Vec<SessionRecord>, String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let mut sql = "SELECT id,station_name,customer,start_time,end_time,duration_minutes,total,payment_method,rate_type,COALESCE(drink_total,0),COALESCE(discount,0),COALESCE(notes,''),COALESCE(tags,''),COALESCE(extra_controllers,0),COALESCE(extra_fee,0) FROM session_history WHERE 1=1".to_string();
    let mut args: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;
    macro_rules! add_filter { ($col:expr, $val:expr) => { if let Some(ref v) = $val { sql.push_str(&format!(" AND {} = ?{}", $col, idx)); args.push(Box::new(v.clone())); idx += 1; } } }
    macro_rules! add_like { ($col:expr, $val:expr) => { if let Some(ref v) = $val { sql.push_str(&format!(" AND {} LIKE ?{}", $col, idx)); args.push(Box::new(format!("%{}%", v))); idx += 1; } } }
    add_filter!("station_name", filter.station_name);
    add_like!("payment_method", filter.payment_method);
    add_like!("customer", filter.customer);
    if let Some(ref sd) = filter.start_date { sql.push_str(&format!(" AND date(end_time) >= ?{}", idx)); args.push(Box::new(sd.clone())); idx += 1; }
    if let Some(ref ed) = filter.end_date { sql.push_str(&format!(" AND date(end_time) <= ?{}", idx)); args.push(Box::new(ed.clone())); idx += 1; }
    if let Some(md) = filter.min_duration { sql.push_str(&format!(" AND duration_minutes >= ?{}", idx)); args.push(Box::new(md)); idx += 1; }
    if let Some(md) = filter.max_duration { sql.push_str(&format!(" AND duration_minutes <= ?{}", idx)); args.push(Box::new(md)); }
    sql.push_str(" ORDER BY end_time DESC LIMIT 1000");
    let refs: Vec<&dyn rusqlite::types::ToSql> = args.iter().map(|a| a.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let records = stmt.query_map(refs.as_slice(), map_history).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).collect();
    Ok(records)
}

#[tauri::command]
pub fn clear_history(state: State<AppState>) -> Result<(), String> {
    crate::commands::auth::require_admin(&state)?;
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    conn.execute("DELETE FROM session_history", []).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM drink_orders", []).map_err(|e| e.to_string())?;
    log_audit_conn(&conn, &state, "clear_history", "history", "Tüm geçmiş silindi");
    Ok(())
}

#[tauri::command]
pub fn delete_history(history_id: String, state: State<AppState>) -> Result<(), String> {
    crate::commands::auth::require_admin(&state)?;
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let (station_name, total): (String, f64) = conn
        .query_row("SELECT station_name, COALESCE(total,0) FROM session_history WHERE id = ?1", params![history_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|_| "Geçmiş kaydı bulunamadı")?;
    conn.execute_batch("BEGIN TRANSACTION").map_err(|e| e.to_string())?;
    let tx_result: Result<(), String> = (|| {
        conn.execute("DELETE FROM drink_orders WHERE session_id = ?1", params![history_id]).map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM partial_payments WHERE session_id = ?1", params![history_id]).map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM session_history WHERE id = ?1", params![history_id]).map_err(|e| e.to_string())?;
        log_audit_conn(&conn, &state, "delete_history", "history", format!("{} (₺{:.2})", station_name, total).as_str());
        Ok(())
    })();
    if let Err(e) = tx_result {
        conn.execute_batch("ROLLBACK").ok();
        return Err(e);
    }
    conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
    Ok(())
}

// ─── Dışa Aktarma ────────────────────────────────────────────────────

#[tauri::command]
pub fn export_history_csv(state: State<AppState>) -> Result<String, String> {
    let records = {
        let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
        let mut stmt = conn.prepare("SELECT station_name,customer,start_time,end_time,duration_minutes,total,payment_method,drink_total,COALESCE(notes,''),COALESCE(tags,'') FROM session_history ORDER BY end_time DESC").map_err(|e| e.to_string())?;
        let rows: Vec<_> = stmt.query_map([], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,String>(3)?,row.get::<_,i64>(4)?,row.get::<_,f64>(5)?,row.get::<_,String>(6)?,row.get::<_,f64>(7)?,row.get::<_,String>(8)?,row.get::<_,String>(9)?))).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).collect();
        rows
    };
    // Lock'u serbest bırak, dosya I/O'su lock altında değil
    let dir = state.data_dir.join("exports");
    fs::create_dir_all(&dir).ok();
    let path = dir.join(format!("gecmis_{}.csv", Local::now().format("%Y%m%d_%H%M%S")));
    let mut wtr = csv::Writer::from_path(&path).map_err(|e| e.to_string())?;
    wtr.write_record(["İstasyon","Müşteri","Başlangıç","Bitiş","Süre(dk)","Toplam(₺)","Ödeme","İçecek(₺)","Notlar","Etiketler"]).map_err(|e| e.to_string())?;
    for (s,c,st,et,d,t,p,dt,n,tg) in &records {
        wtr.write_record([s.as_str(),c.as_str(),st.as_str(),et.as_str(),&d.to_string(),&format!("{:.2}",t),p.as_str(),&format!("{:.2}",dt),n.as_str(),tg.as_str()]).map_err(|e| e.to_string())?;
    }
    wtr.flush().map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn export_history_json(state: State<AppState>) -> Result<String, String> {
    let records = {
        let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
        let mut stmt = conn.prepare("SELECT id,station_name,customer,start_time,end_time,duration_minutes,total,payment_method,rate_type,COALESCE(drink_total,0),COALESCE(discount,0),COALESCE(notes,''),COALESCE(tags,''),COALESCE(extra_controllers,0),COALESCE(extra_fee,0) FROM session_history ORDER BY end_time DESC").map_err(|e| e.to_string())?;
        let rows: Vec<_> = stmt.query_map([], map_history).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).collect();
        rows
    };
    let dir = state.data_dir.join("exports");
    fs::create_dir_all(&dir).ok();
    let path = dir.join(format!("gecmis_{}.json", Local::now().format("%Y%m%d_%H%M%S")));
    fs::write(&path, serde_json::to_string_pretty(&records).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn backup_database(state: State<AppState>) -> Result<String, String> { state.backup_db(); Ok(state.data_dir.join("backups").to_string_lossy().to_string()) }

// ─── Fiş ────────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_receipt(history_id: String, state: State<AppState>) -> Result<ReceiptData, String> {
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;
    let session = conn.query_row(
        "SELECT id,station_name,customer,start_time,end_time,duration_minutes,total,payment_method,rate_type,COALESCE(drink_total,0),COALESCE(discount,0),COALESCE(notes,''),COALESCE(tags,''),COALESCE(extra_controllers,0),COALESCE(extra_fee,0) FROM session_history WHERE id = ?1",
        params![history_id], map_history).map_err(|_| "Kayıt bulunamadı".to_string())?;
    let mut stmt = conn.prepare("SELECT id, session_id, station_name, customer, drink_name, price, quantity, total, order_time FROM drink_orders WHERE session_id = ?1 ORDER BY order_time ASC").map_err(|e| e.to_string())?;
    let drinks: Vec<DrinkOrder> = stmt.query_map(params![history_id], |row| {
        Ok(DrinkOrder {
            id: row.get(0)?, session_id: row.get(1)?, station_name: row.get(2)?, customer: row.get(3)?,
            drink_name: row.get(4)?, price: row.get(5)?, quantity: row.get(6)?, total: row.get(7)?,
            order_time: row.get(8)?,
        })
    }).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).collect();
    Ok(ReceiptData { session, drinks })
}

// ─── Gün Sonu ───────────────────────────────────────────────────────

#[tauri::command]
pub fn get_day_end_report(date: Option<String>, state: State<AppState>) -> Result<DayEndReport, String> {
    let d = date.unwrap_or_else(|| Local::now().format("%Y-%m-%d").to_string());
    let conn = state.db.lock().map_err(|e| format!("DB kilitlenemedi: {}", e))?;

    let (sessions, total_revenue, total_discount, drink_revenue): (i64, f64, f64, f64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(total),0), COALESCE(SUM(discount),0), COALESCE(SUM(drink_total),0) FROM session_history WHERE date(end_time) = ?1",
        params![d], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).map_err(|e| e.to_string())?;

    let cash_revenue: f64 = conn.query_row("SELECT COALESCE(SUM(total),0) FROM session_history WHERE date(end_time) = ?1 AND payment_method LIKE 'nakit%'", params![d], |r| r.get(0)).unwrap_or(0.0);
    let card_revenue: f64 = conn.query_row("SELECT COALESCE(SUM(total),0) FROM session_history WHERE date(end_time) = ?1 AND payment_method LIKE 'kart%'", params![d], |r| r.get(0)).unwrap_or(0.0);
    let other_revenue: f64 = conn.query_row("SELECT COALESCE(SUM(total),0) FROM session_history WHERE date(end_time) = ?1 AND payment_method NOT LIKE 'nakit%' AND payment_method NOT LIKE 'kart%'", params![d], |r| r.get(0)).unwrap_or(0.0);

    let (partial_cash, partial_card): (f64, f64) = conn.query_row(
        "SELECT COALESCE(SUM(CASE WHEN payment_method LIKE '%nakit%' THEN amount ELSE 0 END),0), COALESCE(SUM(CASE WHEN payment_method LIKE '%kart%' THEN amount ELSE 0 END),0) FROM partial_payments WHERE date(created_at) = ?1",
        params![d], |r| Ok((r.get(0)?, r.get(1)?))).unwrap_or((0.0, 0.0));

    let avg_duration_minutes: f64 = conn.query_row("SELECT COALESCE(AVG(duration_minutes),0) FROM session_history WHERE date(end_time) = ?1", params![d], |r| r.get(0)).unwrap_or(0.0);

    let mut top_drinks: Vec<(String, i64, f64)> = Vec::new();
    if let Ok(mut stmt) = conn.prepare("SELECT drink_name, SUM(quantity) q, SUM(total) t FROM drink_orders WHERE date(order_time) = ?1 GROUP BY drink_name ORDER BY t DESC LIMIT 5") {
        if let Ok(rows) = stmt.query_map(params![d], |r| Ok((r.get::<_,String>(0)?, r.get::<_,i64>(1)?, r.get::<_,f64>(2)?))) {
            for r in rows.filter_map(|r| r.ok()) { top_drinks.push(r); }
        }
    }

    let mut top_stations: Vec<(String, i64, f64)> = Vec::new();
    if let Ok(mut stmt) = conn.prepare("SELECT station_name, COUNT(*) c, SUM(total) t FROM session_history WHERE date(end_time) = ?1 GROUP BY station_name ORDER BY t DESC LIMIT 5") {
        if let Ok(rows) = stmt.query_map(params![d], |r| Ok((r.get::<_,String>(0)?, r.get::<_,i64>(1)?, r.get::<_,f64>(2)?))) {
            for r in rows.filter_map(|r| r.ok()) { top_stations.push(r); }
        }
    }

    // Detaylı ürün listesi (gün sonuna kadar satılan tüm ürünler)
    let mut drink_details: Vec<DayEndDrinkDetail> = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT do.drink_name, SUM(do.quantity), SUM(do.total), COALESCE(d.category,''), COALESCE(d.emoji,''), COALESCE(d.price, do.price) \
         FROM drink_orders do LEFT JOIN drinks d ON d.name = do.drink_name \
         WHERE date(do.order_time) = ?1 GROUP BY do.drink_name ORDER BY SUM(do.total) DESC") {
        if let Ok(rows) = stmt.query_map(params![d], |r| Ok(DayEndDrinkDetail {
            name: r.get(0)?, quantity: r.get(1)?, total: r.get(2)?, category: r.get(3)?, emoji: r.get(4)?, price: r.get(5)?,
        })) {
            for r in rows.filter_map(|r| r.ok()) { drink_details.push(r); }
        }
    }

    // Detaylı oturum listesi (gün içinde kapanan tüm oturumlar)
    let mut session_details: Vec<DayEndSessionDetail> = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT station_name, start_time, end_time, duration_minutes, total, payment_method, COALESCE(drink_total,0), COALESCE(discount,0), COALESCE(extra_controllers,0), COALESCE(extra_fee,0), rate_type \
         FROM session_history WHERE date(end_time) = ?1 ORDER BY end_time ASC") {
        if let Ok(rows) = stmt.query_map(params![d], |r| Ok(DayEndSessionDetail {
            station_name: r.get(0)?, start_time: r.get(1)?, end_time: r.get(2)?, duration_minutes: r.get(3)?,
            total: r.get(4)?, payment_method: r.get(5)?, drink_total: r.get(6)?, discount: r.get(7)?,
            extra_controllers: r.get(8)?, extra_fee: r.get(9)?, rate_type: r.get(10)?,
        })) {
            for r in rows.filter_map(|r| r.ok()) { session_details.push(r); }
        }
    }

    Ok(DayEndReport {
        date: d, sessions, total_revenue, total_discount, drink_revenue, avg_duration_minutes,
        cash_revenue, card_revenue, other_revenue, partial_cash, partial_card,
        top_drinks, top_stations, drink_details, session_details,
    })
}
