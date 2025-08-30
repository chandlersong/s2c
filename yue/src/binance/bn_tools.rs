use chrono::{DateTime, Utc};
use std::time::UNIX_EPOCH;

pub fn unix_2_readable(unix_timestamp_millis: &u64) -> DateTime<Utc> {
    // Unix 时间戳（毫秒）

    // 将毫秒转换为秒和纳秒
    let seconds = (unix_timestamp_millis / 1000) as u64;
    let nanoseconds = ((unix_timestamp_millis % 1000) * 1_000_000) as u32;

    // 创建 SystemTime
    let system_time = UNIX_EPOCH + std::time::Duration::new(seconds, nanoseconds);

    system_time.into()
}

#[cfg(test)]
mod tests {
    use crate::binance::bn_tools::unix_2_readable;

    #[test]
    fn test_unix_2_time() {
        let expected = format!("{}", unix_2_readable(&1737093025292));
        assert_eq!("2025-01-17 05:50:25.292 UTC", expected);
    }
}
