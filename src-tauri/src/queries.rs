use chrono::{DateTime, Local};
use rusqlite::{params, Connection};
use serde_json::json;
use std::collections::HashMap;

use crate::db::AppState;
use crate::commands::settings::get_setting_value;

pub fn scalar_i64(conn: &Connection, sql: &str, p: impl rusqlite::Params) -> i64 {
    conn.query_row(sql, p, |r| r.get::<_, i64>(0)).unwrap_or(0)
}

pub fn scalar_f64(conn: &Connection, sql: &str, p: impl rusqlite::Params) -> f64 {
    conn.query_row(sql, p, |r| r.get::<_, f64>(0)).unwrap_or(0.0)
}

/// Ödeme yöntemine göre kullanılacak tarife tipini döndürür.
/// Kısmi ödeme durumunda ("kart+kısmi:...") ana yöntem dikkate alınır.
pub fn rate_type_for<'a>(payment_method: &str, fallback: &'a str) -> &'a str {
    let primary = payment_method.split('+').next().unwrap_or("").trim().to_lowercase();
    if primary.contains("kart") || primary.contains("iban") {
        "kart"
    } else if primary.contains("nakit") {
        "nakit"
    } else {
        fallback
    }
}

pub fn session_fee(
    conn: &Connection,
    _station_id: &str,
    rate_type: &str,
    start_str: &str,
    paused_at: Option<&str>,
    total_paused: i64,
    extra_controllers: i64,
) -> (i64, i64, f64, f64, bool) {
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
    let per_min = if rate_type == "nakit" {
        pricing.cash_per_minute
    } else {
        pricing.card_per_minute
    };
    let round_mins = pricing.round_minutes.max(1);
    let chunks = ((mins as f64) / (round_mins as f64)).ceil() as i64;
    let rounded = chunks * round_mins;
    let fee = (rounded as f64 * per_min).max(pricing.min_charge);
    let extra_per_min = pricing.extra_controller_per_hour / 60.0;
    let extra_fee = extra_controllers.max(0) as f64 * extra_per_min * (rounded as f64);
    (mins, secs, fee, extra_fee, is_paused)
}

pub fn overview(conn: &Connection) -> serde_json::Value {
    let today = Local::now().date_naive().to_string();

    let active = scalar_i64(conn, "SELECT COUNT(*) FROM active_sessions", []);
    let idle = scalar_i64(conn, "SELECT COUNT(*) FROM stations WHERE status = 'idle'", []);
    let total = scalar_i64(conn, "SELECT COUNT(*) FROM stations", []);
    let today_rev = scalar_f64(conn, "SELECT COALESCE(SUM(total),0) FROM session_history WHERE date(end_time) = ?1", params![today]);
    let today_drinks = scalar_f64(conn, "SELECT COALESCE(SUM(total),0) FROM drink_orders WHERE date(order_time) = ?1", params![today]);
    let today_sessions = scalar_i64(conn, "SELECT COUNT(*) FROM session_history WHERE date(end_time) = ?1", params![today]);
    let low_threshold: i64 = get_setting_value(conn, "low_stock_threshold").and_then(|v| v.parse().ok()).unwrap_or(5);

    let mut stations: Vec<serde_json::Value> = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT s.id, s.name, s.station_type, s.group_name, s.status, COALESCE(a.customer,''), a.start_time, a.paused_at, COALESCE(a.total_paused_seconds,0), COALESCE(a.extra_controllers,0) FROM stations s LEFT JOIN active_sessions a ON a.station_id = s.id ORDER BY s.group_name, s.name",
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
                row.get::<_, i64>(9)?,
            ))
        }) {
            for r in rows.flatten() {
                let (id, name, stype, group, status, customer, start, paused_at, total_paused, extra_controllers) = r;
                let eff_status: String = if status == "active" && paused_at.is_some() { "paused".into() } else { status.clone() };
                let elapsed_min = if status == "active" {
                    let (m, _s, _f, _x, _p) = session_fee(conn, &id, "card", start.as_deref().unwrap_or(""), paused_at.as_deref(), total_paused, 0);
                    m
                } else {
                    0
                };
                stations.push(json!({
                    "id": id, "name": name, "type": stype, "group": group,
                    "status": eff_status, "customer": customer, "start_time": start,
                    "elapsed_min": elapsed_min, "extra_controllers": extra_controllers,
                }));
            }
        }
    }

    let mut drink_totals: HashMap<String, f64> = HashMap::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT session_id, COALESCE(SUM(total),0) FROM drink_orders WHERE session_id IN (SELECT station_id FROM active_sessions) GROUP BY session_id",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))) {
            for r in rows.flatten() {
                drink_totals.insert(r.0, r.1);
            }
        }
    }

    let mut sessions: Vec<serde_json::Value> = Vec::new();
    let mut live_estimate = 0.0f64;
    if let Ok(mut stmt) = conn.prepare(
        "SELECT station_id, station_name, customer, start_time, rate_type, paused_at, COALESCE(total_paused_seconds,0), COALESCE(extra_controllers,0) FROM active_sessions",
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
                row.get::<_, i64>(7)?,
            ))
        }) {
            for r in rows.flatten() {
                let (station_id, station_name, customer, start_time, rate_type, paused_at, total_paused, extra_controllers) = r;
                let (mins, _secs, fee, extra_fee, is_paused) =
                    session_fee(conn, &station_id, &rate_type, &start_time, paused_at.as_deref(), total_paused, extra_controllers);
                live_estimate += fee + extra_fee;
                let drink_total = drink_totals.get(&station_id).copied().unwrap_or(0.0);
                sessions.push(json!({
                    "station_id": station_id, "station_name": station_name,
                    "customer": customer, "rate_type": rate_type, "start_time": start_time,
                    "is_paused": is_paused, "minutes": mins, "fee": fee,
                    "extra_controllers": extra_controllers, "extra_fee": extra_fee,
                    "drink_total": drink_total, "total": fee + extra_fee + drink_total,
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

    json!({
        "server_time": Local::now().to_rfc3339(),
        "today": today,
        "summary": { "active": active, "idle": idle, "total": total },
        "today_revenue": today_rev,
        "today_drinks": today_drinks,
        "today_sessions": today_sessions,
        "live_estimate": live_estimate,
        "low_stock_threshold": low_threshold,
        "low_stock": low_stock,
        "stations": stations,
        "sessions": sessions,
    })
}

pub fn drinks(conn: &Connection) -> serde_json::Value {
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
    json!(items)
}

pub fn history(conn: &Connection, limit: i64) -> serde_json::Value {
    let mut items: Vec<serde_json::Value> = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT id, station_name, customer, start_time, end_time, duration_minutes, total, payment_method, COALESCE(drink_total,0) FROM session_history ORDER BY end_time DESC LIMIT ?1",
    ) {
        if let Ok(rows) = stmt.query_map(params![limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, f64>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, f64>(8)?,
            ))
        }) {
            for r in rows.flatten() {
                let (id, station_name, customer, start_time, end_time, duration_minutes, total, payment_method, drink_total) = r;
                items.push(json!({
                    "id": id,
                    "station_name": station_name, "customer": customer,
                    "start_time": start_time, "end_time": end_time,
                    "duration_minutes": duration_minutes, "total": total,
                    "payment_method": payment_method, "drink_total": drink_total,
                }));
            }
        }
    }
    json!(items)
}

pub fn history_since_days(conn: &Connection, days: i64) -> serde_json::Value {
    let cutoff = (Local::now() - chrono::Duration::days(days)).to_rfc3339();
    let mut items: Vec<serde_json::Value> = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT id, station_name, customer, start_time, end_time, duration_minutes, total, payment_method, COALESCE(drink_total,0), COALESCE(extra_controllers,0), COALESCE(extra_fee,0) FROM session_history WHERE end_time >= ?1 ORDER BY end_time DESC",
    ) {
        if let Ok(rows) = stmt.query_map(params![cutoff], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, f64>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, f64>(8)?,
                row.get::<_, i64>(9)?,
                row.get::<_, f64>(10)?,
            ))
        }) {
            for r in rows.flatten() {
                let (id, station_name, customer, start_time, end_time, duration_minutes, total, payment_method, drink_total, extra_controllers, extra_fee) = r;
                items.push(json!({
                    "id": id,
                    "station_name": station_name, "customer": customer,
                    "start_time": start_time, "end_time": end_time,
                    "duration_minutes": duration_minutes, "total": total,
                    "payment_method": payment_method, "drink_total": drink_total,
                    "extra_controllers": extra_controllers, "extra_fee": extra_fee,
                }));
            }
        }
    }
    json!(items)
}

pub fn day_end(conn: &Connection, date: &str) -> serde_json::Value {
    let (sessions, total_revenue, total_discount, drink_revenue): (i64, f64, f64, f64) = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(total),0), COALESCE(SUM(discount),0), COALESCE(SUM(drink_total),0) FROM session_history WHERE date(end_time) = ?1",
            params![date],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap_or((0, 0.0, 0.0, 0.0));

    let cash_revenue = scalar_f64(
        conn,
        "SELECT COALESCE(SUM(total),0) FROM session_history WHERE date(end_time) = ?1 AND payment_method LIKE '%nakit%'",
        params![date],
    );
    let card_revenue = scalar_f64(
        conn,
        "SELECT COALESCE(SUM(total),0) FROM session_history WHERE date(end_time) = ?1 AND payment_method LIKE '%kart%'",
        params![date],
    );
    let other_revenue = scalar_f64(
        conn,
        "SELECT COALESCE(SUM(total),0) FROM session_history WHERE date(end_time) = ?1 AND payment_method NOT LIKE '%nakit%' AND payment_method NOT LIKE '%kart%'",
        params![date],
    );
    let partial_cash = scalar_f64(
        conn,
        "SELECT COALESCE(SUM(CASE WHEN payment_method LIKE '%nakit%' THEN amount ELSE 0 END),0) FROM partial_payments WHERE date(created_at) = ?1",
        params![date],
    );
    let partial_card = scalar_f64(
        conn,
        "SELECT COALESCE(SUM(CASE WHEN payment_method LIKE '%kart%' THEN amount ELSE 0 END),0) FROM partial_payments WHERE date(created_at) = ?1",
        params![date],
    );
    let avg_duration_minutes = scalar_f64(
        conn,
        "SELECT COALESCE(AVG(duration_minutes),0) FROM session_history WHERE date(end_time) = ?1",
        params![date],
    );

    let mut top_drinks: Vec<serde_json::Value> = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT drink_name, SUM(quantity) q, SUM(total) t FROM drink_orders WHERE date(order_time) = ?1 GROUP BY drink_name ORDER BY t DESC LIMIT 5",
    ) {
        if let Ok(rows) = stmt.query_map(params![date], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, f64>(2)?))
        }) {
            for r in rows.flatten() {
                let (name, quantity, total) = r;
                top_drinks.push(json!({ "name": name, "quantity": quantity, "total": total }));
            }
        }
    }

    let mut top_stations: Vec<serde_json::Value> = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT station_name, COUNT(*) c, SUM(total) t FROM session_history WHERE date(end_time) = ?1 GROUP BY station_name ORDER BY t DESC LIMIT 5",
    ) {
        if let Ok(rows) = stmt.query_map(params![date], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, f64>(2)?))
        }) {
            for r in rows.flatten() {
                let (name, sessions, total) = r;
                top_stations.push(json!({ "name": name, "sessions": sessions, "total": total }));
            }
        }
    }

    json!({
        "date": date,
        "sessions": sessions,
        "total_revenue": total_revenue,
        "total_discount": total_discount,
        "drink_revenue": drink_revenue,
        "avg_duration_minutes": avg_duration_minutes,
        "cash_revenue": cash_revenue,
        "card_revenue": card_revenue,
        "other_revenue": other_revenue,
        "partial_cash": partial_cash,
        "partial_card": partial_card,
        "top_drinks": top_drinks,
        "top_stations": top_stations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrate_db;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate_db(&conn);
        conn.execute_batch(
            "INSERT INTO stations (id, name, station_type, status, group_name) VALUES ('pc-test', 'Test', 'standard', 'active', '')",
        )
        .unwrap();
        conn
    }

    #[test]
    fn paused_session_freezes_minutes() {
        let conn = test_conn();
        let start = (Local::now() - chrono::Duration::minutes(50)).to_rfc3339();
        let paused = (Local::now() - chrono::Duration::minutes(20)).to_rfc3339();
        let (mins, _s, _f, _x, is_paused) = session_fee(&conn, "pc-test", "nakit", &start, Some(&paused), 0, 0);
        assert!(is_paused);
        assert!(mins >= 28 && mins <= 32, "beklenen ~30 dk, gelen {}", mins);
    }

    #[test]
    fn resumed_session_subtracts_total_paused() {
        let conn = test_conn();
        let start = (Local::now() - chrono::Duration::minutes(50)).to_rfc3339();
        let (mins, _s, _f, _x, is_paused) = session_fee(&conn, "pc-test", "nakit", &start, None, 1200, 0);
        assert!(!is_paused);
        assert!(mins >= 28 && mins <= 32, "beklenen ~30 dk, gelen {}", mins);
    }

    #[test]
    fn extra_controllers_add_hourly_fee_per_minute() {
        let conn = test_conn();
        let start = (Local::now() - chrono::Duration::minutes(30)).to_rfc3339();
        let (mins, _s, _f, extra_fee, _p) = session_fee(&conn, "pc-test", "nakit", &start, None, 0, 1);
        assert!(mins >= 29 && mins <= 31, "beklenen ~30 dk, gelen {}", mins);
        let expected = 75.0 / 60.0 * mins as f64;
        assert!((extra_fee - expected).abs() < 0.01, "beklenen ~{:.2}, gelen {:.2}", expected, extra_fee);
    }

    #[test]
    fn paused_station_is_reported_paused_in_overview() {
        let conn = test_conn();
        conn.execute(
            "INSERT INTO active_sessions (station_id, station_name, customer, start_time, rate_type) VALUES ('pc-test', 'Test', 'Musteri', ?1, 'nakit')",
            params![(Local::now() - chrono::Duration::minutes(50)).to_rfc3339()],
        )
        .unwrap();
        conn.execute("UPDATE active_sessions SET paused_at = ?1 WHERE station_id = 'pc-test'", params![(Local::now() - chrono::Duration::minutes(20)).to_rfc3339()])
            .unwrap();
        let ov = overview(&conn);
        let stations = ov["stations"].as_array().unwrap();
        let s = stations.iter().find(|x| x["id"] == "pc-test").unwrap();
        assert_eq!(s["status"], "paused");
    }

    #[test]
    fn rate_type_reflects_payment_method() {
        assert_eq!(rate_type_for("kart", "nakit"), "kart");
        assert_eq!(rate_type_for("nakit", "kart"), "nakit");
        assert_eq!(rate_type_for("iban", "nakit"), "kart");
        assert_eq!(rate_type_for("kart+kısmi:nakit:50.00", "nakit"), "kart");
        assert_eq!(rate_type_for("nakit+kısmi:kart:50.00", "kart"), "nakit");
        assert_eq!(rate_type_for("", "kart"), "kart");
    }
}
