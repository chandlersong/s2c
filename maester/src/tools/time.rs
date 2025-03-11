use chrono::{DateTime, Datelike, Duration as ChronoDuration, TimeZone, Utc, };
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

pub fn get_next_utc_day_begin() -> Instant {
    let now = Utc::now();
    // 计算今天的 00:00 UTC
    let today_midnight = Utc
        .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
        .single()
        .expect("Failed to create UTC midnight");

    // 计算今天的 24:00（即下一天的 00:00）
    let today_end = today_midnight + ChronoDuration::days(1);

    // 使用 UNIX_EPOCH 作为基准
    let unix_epoch = chrono::DateTime::<Utc>::UNIX_EPOCH;

    // 计算时间差
    let now_duration = now - unix_epoch;
    let today_end_duration = today_end - unix_epoch;

    // 转换为 Instant
    let instant_now = Instant::now();
    instant_now + (today_end_duration - now_duration).to_std().expect("Duration out of range")
}
