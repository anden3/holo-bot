use tracing::instrument;

#[tokio::main(flavor = "multi_thread")]
#[instrument]
async fn main() -> anyhow::Result<()> {
    lib::HoloBot::start().await
}
