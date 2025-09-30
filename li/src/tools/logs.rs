use log::LevelFilter;
use std::collections::HashMap;
use std::time::SystemTime;

//PLAN: 以后加入一些分布式的log库

pub fn setup_logger_all(log_level: Option<LevelFilter>) -> Result<(), fern::InitError> {
    setup_logger(log_level, HashMap::new())?;
    Ok(())
}

pub fn setup_logger(default_level: Option<LevelFilter>, special_level: HashMap<String, LevelFilter>) -> Result<(), fern::InitError> {
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
