use log::LevelFilter;
use std::time::SystemTime;

pub fn setup_logger(level: Option<LevelFilter>) -> Result<(), fern::InitError> {
    let filter = match level {
        None => { LevelFilter::Debug }
        Some(v) => { v }
    };
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{} {} {} {}:{}] {}",
                humantime::format_rfc3339_seconds(SystemTime::now()),
                record.level(),
                record.target(),
                line!(),
                column!(),
                message
            ))
        })
        .level(filter)
        .chain(std::io::stdout())
        .apply()?;
    Ok(())
}
