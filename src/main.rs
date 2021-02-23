mod lib;

use crate::lib::TwitterScraper;

#[tokio::main]
async fn main() {
    TwitterScraper::start().await;
}
