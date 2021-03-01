mod lib;

use crate::lib::HoloBot;
use tokio::runtime::Runtime;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rt = Runtime::new()?;

    rt.block_on(async {
        HoloBot::start().await;
    });

    Ok(())
}
