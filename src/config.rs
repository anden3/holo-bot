use std::fs;

use chrono::prelude::*;
use serde::Deserialize;
use serde_hex::{SerHex, StrictPfx};
use url::Url;

#[derive(Deserialize, Clone)]
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

    pub live_notif_channel: u64,
    pub schedule_channel: u64,
    pub birthday_notif_channel: u64,

    pub users: Vec<User>,
}

impl Config {
    pub fn load_config(path: &str) -> Self {
        let config_json = fs::read_to_string(path).expect("Something went wrong reading the file.");
        return serde_json::from_str(&config_json).expect("Couldn't parse config.");
    }
}

#[derive(Deserialize, Clone)]
pub struct User {
    pub name: String,
    pub display_name: String,
    pub icon: Url,
    pub channel: String,

    pub birthday: (u32, u32),
    #[serde(with = "super::serializers::timezone")]
    pub timezone: chrono_tz::Tz,

    pub twitter_handle: String,
    pub twitter_id: u64,
    pub schedule_keyword: String,

    #[serde(with = "SerHex::<StrictPfx>")]
    pub colour: u32,
    pub discord_role: u64,
}

impl User {
    pub fn get_next_birthday(&self) -> DateTime<Utc> {
        let now = Utc::now();
        let mut year = now.year();

        let (day, month) = self.birthday;

        if month > now.month() || day > now.day() {
            year += 1;
        }

        self.timezone
            .ymd(year, month, day)
            .and_hms(12, 0, 0)
            .with_timezone(&Utc)
    }
}
