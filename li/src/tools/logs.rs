use log::LevelFilter;
use std::time::SystemTime;

pub fn setup_logger(level: Option<LevelFilter>) -> Result<(), fern::InitError> {
    let filter = level.unwrap_or_else(|| LevelFilter::Debug);
    fern::Dispatch::new()
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
        .chain(std::io::stdout())
        .apply()?;
    Ok(())
}
