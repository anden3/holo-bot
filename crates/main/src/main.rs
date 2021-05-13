#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    lib::HoloBot::start().await
}
