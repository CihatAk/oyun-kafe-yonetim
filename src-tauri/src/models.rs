use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Station {
    pub id: String,
    pub name: String,
    pub station_type: String,
    pub status: String,
    pub group_name: String,
    pub total_sessions: i64,
    pub total_revenue: f64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ActiveSession {
    pub station_id: String,
    pub station_name: String,
    pub customer: String,
    pub start_time: String,
    pub rate_type: String,
    pub notes: String,
    pub tags: String,
    pub paused_at: Option<String>,
    pub total_paused_seconds: i64,
    pub extra_controllers: i64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SessionRecord {
    pub id: String,
    pub station_name: String,
    pub customer: String,
    pub start_time: String,
    pub end_time: String,
    pub duration_minutes: i64,
    pub total: f64,
    pub payment_method: String,
    pub rate_type: String,
    pub drink_total: f64,
    pub discount: f64,
    pub notes: String,
    pub tags: String,
    pub extra_controllers: i64,
    pub extra_fee: f64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct DrinkItem {
    pub id: String,
    pub name: String,
    pub price: f64,
    pub category: String,
    pub stock: i64,
    pub emoji: String,
    pub description: String,
    pub cost: f64,
    pub min_stock: i64,
    pub is_active: i64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct DrinkOrder {
    pub id: String,
    pub session_id: String,
    pub station_name: String,
    pub customer: String,
    pub drink_name: String,
    pub price: f64,
    pub quantity: i32,
    pub total: f64,
    pub order_time: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct StockMovement {
    pub id: String,
    pub drink_id: String,
    pub drink_name: String,
    pub change_amount: i64,
    pub stock_after: i64,
    pub reason: String,
    pub created_at: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PricingConfig {
    pub cash_per_minute: f64,
    pub card_per_minute: f64,
    pub min_charge: f64,
    pub round_minutes: i64,
    pub extra_controller_per_hour: f64,
    pub max_session_minutes: i64,
    pub warning_before_minutes: i64,
}

impl Default for PricingConfig {
    fn default() -> Self {
        PricingConfig {
            cash_per_minute: 4.20,
            card_per_minute: 5.00,
            min_charge: 0.0,
            round_minutes: 1,
            extra_controller_per_hour: 75.00,
            max_session_minutes: 0,
            warning_before_minutes: 5,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct HistoryFilter {
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub station_name: Option<String>,
    pub payment_method: Option<String>,
    pub customer: Option<String>,
    pub min_duration: Option<i64>,
    pub max_duration: Option<i64>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct UserRecord {
    pub id: String,
    pub username: String,
    pub full_name: String,
    pub role: String,
    pub active: i64,
    pub permissions: String,
    pub created_at: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AuditRecord {
    pub id: String,
    pub user_name: String,
    pub action: String,
    pub entity: String,
    pub detail: String,
    pub created_at: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ReceiptData {
    pub session: SessionRecord,
    pub drinks: Vec<DrinkOrder>,
}

#[derive(Clone, Serialize, Debug)]
pub struct DayEndReport {
    pub date: String,
    pub sessions: i64,
    pub total_revenue: f64,
    pub total_discount: f64,
    pub drink_revenue: f64,
    pub avg_duration_minutes: f64,
    pub cash_revenue: f64,
    pub card_revenue: f64,
    pub other_revenue: f64,
    pub partial_cash: f64,
    pub partial_card: f64,
    pub top_drinks: Vec<(String, i64, f64)>,
    pub top_stations: Vec<(String, i64, f64)>,
}
