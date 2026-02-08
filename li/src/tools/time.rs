use chrono::{DateTime, Datelike, Duration as ChronoDuration, TimeZone, Timelike, Utc};
use std::time::UNIX_EPOCH;
use tokio::time::Instant;

pub const ONE_HOUR_MS: u64 = 60 * 60 * 1000;
pub const ONE_MILL_SECOND_MS: u64 = 1;

pub const GENESIS_2020_MS: u64 = 1577836800000;
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

pub fn get_prev_utc_hour_end() -> u64 {
    // 获取当前 UTC 时间
    let now = Utc::now();

    // 构造当前小时的开始时间（分钟和秒为0）
    let current_hour_start = Utc
        .with_ymd_and_hms(now.year(), now.month(), now.day(), now.hour(), 0, 0)
        .single()
        .expect("Failed to create current hour start");

    // previous hour's end corresponds to current_hour_start
    // 返回毫秒时间戳，保持与项目中其他时间戳格式一致
    current_hour_start.timestamp_millis() as u64
}

pub fn get_next_utc_hour_begin() -> Instant {
    let now = Utc::now();
    // 计算今���的 00:00 UTC
    let current_hour = Utc
        .with_ymd_and_hms(now.year(), now.month(), now.day(), now.hour(), 0, 0)
        .single()
        .expect("Failed to create UTC midnight");

    // 计算今天的 24:00（即下一天的 00:00）
    let today_end = current_hour + ChronoDuration::hours(1);

    // 使用 UNIX_EPOCH 作为基准
    let unix_epoch = chrono::DateTime::<Utc>::UNIX_EPOCH;

    // 计算时间差
    let now_duration = now - unix_epoch;
    let today_end_duration = today_end - unix_epoch;

    // 转换为 Instant
    let instant_now = Instant::now();
    instant_now + (today_end_duration - now_duration).to_std().expect("Duration out of range")
}

pub fn unix_2_readable(unix_timestamp_millis: &u64) -> DateTime<Utc> {
    // Unix 时间戳（毫秒）

    // 将毫秒转换为秒和纳秒
    let seconds = (unix_timestamp_millis / 1000) as u64;
    let nanoseconds = ((unix_timestamp_millis % 1000) * 1_000_000) as u32;

    // 创建 SystemTime
    let system_time = UNIX_EPOCH + std::time::Duration::new(seconds, nanoseconds);

    system_time.into()
}

pub type UnixTimeStamp = u64;

pub fn unix_time_now_u64_utc() -> UnixTimeStamp {
    let now = Utc::now();
    now.timestamp_millis() as u64
}

#[cfg(test)]
mod tests {
    use crate::tools::time::{get_next_utc_day_begin, get_next_utc_hour_begin, get_prev_utc_hour_end, instant_to_datetime, unix_2_readable};
    use chrono::{Datelike, TimeZone, Timelike, Utc};
    use tokio::time::Instant;

    #[test]
    pub fn test_get_next_utc_day_begin() {
        let now = Instant::now();
        let actual = get_next_utc_day_begin();
        let duration = actual - now;
        assert!(duration.as_secs() < 60 * 60 * 24);
        assert!(actual > now);
        let datetime = instant_to_datetime(actual).to_utc();
        assert_eq!(datetime.hour(), 0);
        assert_eq!(datetime.minute(), 0);
        assert_eq!(datetime.second(), 0);
    }

    #[test]
    pub fn test_get_next_utc_hour_begin() {
        let now = Instant::now();
        let actual = get_next_utc_hour_begin();
        let duration = actual - now;
        assert!(duration.as_secs() < 60 * 60);
        assert!(actual > now);
        let datetime = instant_to_datetime(actual).to_utc();
        println!("next hour is {}", datetime);
        assert_eq!(datetime.minute(), 0);
        assert_eq!(datetime.second(), 0);
    }

    #[test]
    pub fn test_get_prev_utc_hour_end() {
        // 计算期望的当前小时开始时间（即“前一小时的结束”）
        let now = Utc::now();
        let current_hour_start = Utc
            .with_ymd_and_hms(now.year(), now.month(), now.day(), now.hour(), 0, 0)
            .single()
            .expect("Failed to create current hour start");

        let expected = current_hour_start.timestamp_millis() as u64;
        let actual = get_prev_utc_hour_end();
        let dt = Utc.timestamp_millis_opt(actual as i64);
        println!("{:?}", dt);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_unix_2_time() {
        let expected = format!("{}", unix_2_readable(&1737093025292));
        assert_eq!("2025-01-17 05:50:25.292 UTC", expected);
    }
}
