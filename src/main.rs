mod lib;

use crate::lib::HoloBot;

#[tokio::main]
async fn main() {
    let bot = HoloBot::new().await;
    bot.start().await.unwrap();
}
