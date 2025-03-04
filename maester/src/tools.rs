
pub mod times {
    use chrono::{DateTime, Utc};

    pub fn current_date_string() -> String {
        let now: DateTime<Utc> = Utc::now();
        now.format("%Y-%m-%d").to_string()
    }
}