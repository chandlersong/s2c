use crate::errors::LiError;
use log::LevelFilter;
use std::collections::HashMap;
use std::time::SystemTime;
//PLAN: 以后加入一些分布式的log库

pub fn setup_logger_all(log_level: Option<LevelFilter>) -> Result<(), LiError> {
    setup_logger(log_level, HashMap::new())?;
    Ok(())
}

pub fn parse_level(level: Option<&str>) -> LevelFilter {
    match level.map(|s| s.to_ascii_lowercase()) {
        Some(ref s) if s == "off" => LevelFilter::Off,
        Some(ref s) if s == "error" => LevelFilter::Error,
        Some(ref s) if s == "warn" || s == "warning" => LevelFilter::Warn,
        Some(ref s) if s == "info" => LevelFilter::Info,
        Some(ref s) if s == "debug" => LevelFilter::Debug,
        Some(ref s) if s == "trace" => LevelFilter::Trace,
        _ => LevelFilter::Warn,
    }
}

pub fn setup_logger(default_level: Option<LevelFilter>, special_level: HashMap<String, LevelFilter>) -> Result<(), LiError> {
    let filter = default_level.unwrap_or_else(|| LevelFilter::Debug);
    let mut logger_builder = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{} {} {}] {}",
                humantime::format_rfc3339_seconds(SystemTime::now()),
                record.level(),
                record.target(),
                message
            ))
        })
        .level(filter)
        .chain(std::io::stdout());

    for (module, level) in special_level {
        logger_builder = logger_builder.level_for(module, level);
    }

    logger_builder.apply()?;
    Ok(())
}
