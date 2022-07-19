use tracing::{error, Level};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{filter::EnvFilter, fmt, prelude::*};

pub struct Logger {}

impl Logger {
    pub fn initialize() -> anyhow::Result<Option<WorkerGuard>> {
        let logging_guard = Self::set_subscriber()?;

        std::panic::set_hook(Box::new(|panic| {
            // If the panic has a source location, record it as structured fields.
            panic.location().map_or_else(
                || {
                    error!(message = %panic);
                },
                |location| {
                    error!(
                        message = %panic,
                        panic.file = location.file(),
                        panic.line = location.line(),
                        panic.column = location.column(),
                    );
                },
            );
        }));

        Ok(logging_guard)
    }

    #[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
    fn set_subscriber() -> anyhow::Result<Option<WorkerGuard>> {
        std::fs::create_dir_all("logs")?;

        let file_appender = tracing_appender::rolling::daily("logs", "output.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        let filter = EnvFilter::from_default_env()
            .add_directive("surf::middleware::logger=error".parse()?)
            .add_directive("serenity::client::bridge=warn".parse()?)
            .add_directive(Level::INFO.into());

        tracing_subscriber::registry()
            .with(
                fmt::Layer::new()
                    .with_writer(non_blocking)
                    .with_filter(filter),
            )
            .with(
                fmt::Layer::new()
                    .with_ansi(true)
                    .with_writer(std::io::stdout)
                    .without_time()
                    .with_filter(filter),
            )
            .init();

        Ok(Some(guard))
    }

    #[cfg(target_arch = "x86_64")]
    fn set_subscriber() -> anyhow::Result<Option<WorkerGuard>> {
        //         let console_layer = console_subscriber::ConsoleLayer::builder()
        //             .with_default_env()
        //             .spawn();

        let filter = EnvFilter::from_default_env()
            .add_directive("surf::middleware::logger=error".parse()?)
            .add_directive("serenity::client::bridge=warn".parse()?)
            // .add_directive("utility::config=debug".parse()?)
            // .add_directive("holodex=debug".parse()?)
            .add_directive("commands::music=trace".parse()?)
            .add_directive("music_queue=trace".parse()?)
            .add_directive("[]=error".parse()?)
            .add_directive("ureq=info".parse()?)
            .add_directive("rustls=info".parse()?)
            .add_directive("h2=info".parse()?)
            .add_directive("reqwest=info".parse()?)
            .add_directive("hyper=info".parse()?)
            .add_directive(Level::DEBUG.into());

        tracing_subscriber::registry()
            // .with(console_layer)
            .with(
                fmt::Layer::new()
                    .with_ansi(true)
                    .with_writer(std::io::stdout)
                    .pretty()
                    .with_filter(filter),
            )
            .init();

        Ok(None)
    }
}