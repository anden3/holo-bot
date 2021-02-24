use std::fs;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    #[serde(rename = "api_key")]
    _api_key: String,
    #[serde(rename = "api_secret")]
    _api_secret: String,
    #[serde(rename = "access_token")]
    _access_token: String,
    #[serde(rename = "access_token_secret")]
    _access_token_secret: String,

    pub bearer_token: String,
    pub discord_token: String,

    pub users: Vec<User>,
}

impl Config {
    pub fn load_config(path: &str) -> Self {
        let config_json = fs::read_to_string(path).expect("Something went wrong reading the file.");
        return serde_json::from_str(&config_json).expect("Couldn't parse config.");
    }
}

#[derive(Deserialize)]
pub struct User {
    pub name: String,

    pub twitter_handle: String,
    pub twitter_id: u64,
    pub schedule_keyword: String,
    pub colour: String,
}
