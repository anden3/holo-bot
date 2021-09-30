use tracing::{error, Level};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

pub struct Logger {}

impl Logger {
    pub fn initialize() -> anyhow::Result<Option<WorkerGuard>> {
        let possible_guard = Self::set_subscriber()?;

        std::panic::set_hook(Box::new(|panic| {
            // If the panic has a source location, record it as structured fields.
            if let Some(location) = panic.location() {
                error!(
                    message = %panic,
                    panic.file = location.file(),
                    panic.line = location.line(),
                    panic.column = location.column(),
                );
            } else {
                error!(message = %panic);
            }
        }));

        Ok(possible_guard)
    }

    #[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
    fn set_subscriber() -> anyhow::Result<Option<WorkerGuard>> {
        std::fs::create_dir_all("logs")?;

        let file_appender = tracing_appender::rolling::daily("logs", "output.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        let filter = EnvFilter::from_default_env()
            .add_directive("surf::middleware::logger=error".parse()?)
            .add_directive("serenity::client::bridge=warn".parse()?)
            .add_directive("apis::mchad_api=warn".parse()?)
            .add_directive("commands::music=debug".parse()?)
            .add_directive(Level::INFO.into());

        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::Layer::new().with_writer(non_blocking))
            .with(
                fmt::Layer::new()
                    .with_writer(std::io::stdout)
                    .without_time(),
            )
            .init();

        Ok(Some(guard))
    }

    #[cfg(target_arch = "x86_64")]
    fn set_subscriber() -> anyhow::Result<Option<WorkerGuard>> {
        let filter = EnvFilter::from_default_env()
            .add_directive("surf::middleware::logger=error".parse()?)
            .add_directive("serenity::client::bridge=warn".parse()?)
            .add_directive("apis::mchad_api=warn".parse()?)
            .add_directive("commands::music=debug".parse()?)
            .add_directive(Level::INFO.into());

        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::Layer::new().with_writer(std::io::stdout).pretty())
            .init();

        Ok(None)
    }
}
