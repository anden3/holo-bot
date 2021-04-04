use log::LevelFilter;

pub struct Logger {}

impl Logger {
    pub fn initialize() -> Result<(), fern::InitError> {
        let colours = fern::colors::ColoredLevelConfig::new();

        fern::Dispatch::new()
            .format(move |out, message, record| {
                out.finish(format_args!(
                    "{}[{}][{}] {}",
                    chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
                    record.target(),
                    colours.color(record.level()),
                    message,
                ));
            })
            .chain(
                fern::Dispatch::new()
                    .level(LevelFilter::Info)
                    .level_for("holo_bot", LevelFilter::Debug)
                    .level_for("serenity", LevelFilter::Warn)
                    .level_for("tracing", LevelFilter::Warn)
                    .chain(std::io::stdout()),
            )
            .chain(
                fern::Dispatch::new()
                    .level(LevelFilter::Error)
                    .chain(std::io::stderr()),
            )
            .chain(
                fern::Dispatch::new()
                    .level(LevelFilter::Info)
                    .chain(fern::log_file("holo-bot.log")?)
                    .chain(
                        std::fs::OpenOptions::new()
                            .write(true)
                            .create(true)
                            .truncate(true)
                            .create(true)
                            .open("/tmp/holo-bot.log")?,
                    ),
            )
            .apply()?;

        Ok(())
    }
}
