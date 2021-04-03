use std::{io::stdout, sync::Mutex};

use crossterm::{
    cursor, execute,
    terminal::{self, ClearType},
};
use lazy_static::lazy_static;
use log::LevelFilter;

pub struct Logger {}

impl Logger {
    pub fn initialize() -> Result<(), fern::InitError> {
        let colours = fern::colors::ColoredLevelConfig::new();

        fern::Dispatch::new()
            .format(move |out, message, record| {
                lazy_static! {
                    static ref LAST_TARGET: Mutex<String> = Mutex::new(String::new());
                }

                match record.target() {
                    "holo_bot::lib::holo_api"
                        if record.level() == LevelFilter::Debug
                            && *LAST_TARGET.lock().unwrap() == "holo_bot::lib::holo_api" =>
                    {
                        execute!(
                            stdout(),
                            cursor::MoveUp(1),
                            cursor::MoveToColumn(0),
                            terminal::Clear(ClearType::CurrentLine)
                        )
                        .unwrap();
                    }
                    _ => (),
                };

                out.finish(format_args!(
                    "{}[{}][{}] {}",
                    chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
                    record.target(),
                    colours.color(record.level()),
                    message,
                ));

                *LAST_TARGET.lock().unwrap() = record.target().to_string();
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
                    .level(LevelFilter::Debug)
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