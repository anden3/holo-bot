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
                    .level_for("apis", LevelFilter::Debug)
                    .level_for("bot", LevelFilter::Debug)
                    .level_for("commands", LevelFilter::Trace)
                    .level_for("lib", LevelFilter::Debug)
                    .level_for("main", LevelFilter::Debug)
                    .level_for("utility", LevelFilter::Debug)
                    .level_for("serenity", LevelFilter::Warn)
                    .level_for("tracing", LevelFilter::Warn)
                    .level_for("ureq::unit", LevelFilter::Warn)
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
                    .level_for("tracing", LevelFilter::Warn)
                    .level_for("ureq::unit", LevelFilter::Warn)
                    .level_for("serenity::gateway::shard", LevelFilter::Warn)
                    .level_for("serenity::http::client", LevelFilter::Warn)
                    .level_for("serenity::http::request", LevelFilter::Warn)
                    .level_for("serenity::gateway::ws_client_ext", LevelFilter::Warn)
                    .level_for("serenity::client::dispatch", LevelFilter::Warn)
                    .level_for("serenity::http::ratelimiting", LevelFilter::Warn)
                    .chain(fern::log_file("holo-bot.log")?),
            )
            .apply()?;

        Ok(())
    }
}
