#[path = "twitter_api.rs"]
mod twitter_api;

use futures::StreamExt;
use reqwest::Error;
use serde::Deserialize;
use std::fs;
use twitter_api::User;

#[derive(Deserialize)]
struct Config {
    #[serde(rename = "api_key")]
    _api_key: String,
    #[serde(rename = "api_secret")]
    _api_secret: String,
    #[serde(rename = "access_token")]
    _access_token: String,
    #[serde(rename = "access_token_secret")]
    _access_token_secret: String,
    bearer_token: String,

    users: Vec<User>,
}

impl Config {
    pub fn load_config(path: &str) -> Self {
        let config_json = fs::read_to_string(path).expect("Something went wrong reading the file.");
        return serde_json::from_str(&config_json).expect("Couldn't parse config.");
    }
}
pub struct TwitterScraper {
    config: Config,
}

impl TwitterScraper {
    pub async fn start() {
        let config = Config::load_config("settings.json");

        let ts = TwitterScraper { config };
        ts.run().await.unwrap();
    }

    async fn run(&self) -> Result<(), Error> {
        let api = twitter_api::TwitterAPI::new(&self.config.bearer_token);
        api.setup_rules(&self.config.users).await.unwrap();

        let mut stream = api.connect().await.unwrap();

        while let Some(item) = stream.next().await {
            let response = item.unwrap();

            if response == "\r\n" {
                continue;
            }

            let response: serde_json::Value =
                serde_json::from_slice(&response).expect("Deserialization of response failed.");

            println!("Response: {:#?}", response);
        }

        Ok(())
    }
}
