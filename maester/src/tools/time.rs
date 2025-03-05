use chrono::{DateTime, Utc};
use tokio::time::Instant;

pub fn current_date_string() -> String {
    let now: DateTime<Utc> = Utc::now();
    now.format("%Y-%m-%d").to_string()
}

pub fn instant_to_datetime(instant: Instant) -> DateTime<Utc> {
    // 获取当前时间作为基准
    let now_instant = Instant::now();
    let now_utc = Utc::now();

    // 计算时间差
    let duration = if instant >= now_instant {
        instant - now_instant
    } else {
        now_instant - instant
    };

    // 将时间差应用于当前 UTC 时间
    if instant >= now_instant {
        now_utc + chrono::Duration::from_std(duration).expect("Duration out of range")
    } else {
        now_utc - chrono::Duration::from_std(duration).expect("Duration out of range")
    }
}
