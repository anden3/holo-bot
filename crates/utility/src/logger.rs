use tracing::{error, Level};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

pub struct Logger {}

impl Logger {
    pub fn initialize() -> anyhow::Result<()> {
        Self::set_subscriber()?;

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

        Ok(())
    }

    #[cfg(target_arch = "arm")]
    fn set_subscriber() -> anyhow::Result<()> {
        std::fs::create_dir_all("logs")?;

        let file_appender = tracing_appender::rolling::daily("logs", "output.log");
        let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

        let filter = EnvFilter::from_default_env()
            .add_directive("surf::middleware::logger=error".parse()?)
            .add_directive("serenity::client::bridge=warn".parse()?)
            .add_directive(Level::INFO.into());

        tracing_subscriber::registry()
            .with(filter)
            .with(
                fmt::Layer::new()
                    .with_writer(non_blocking)
                    .pretty()
                    .without_time(),
            )
            .with(
                fmt::Layer::new()
                    .with_writer(std::io::stdout)
                    .without_time(),
            )
            .init();

        Ok(())
    }

    #[cfg(target_arch = "x86_64")]
    fn set_subscriber() -> anyhow::Result<()> {
        let filter = EnvFilter::from_default_env()
            .add_directive("surf::middleware::logger=error".parse()?)
            .add_directive("serenity::client::bridge=warn".parse()?)
            // .add_directive("apis::holo_api=debug".parse()?)
            .add_directive(Level::INFO.into());

        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::Layer::new().with_writer(std::io::stdout).pretty())
            .init();

        Ok(())
    }
}
