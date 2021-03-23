use std::convert::TryFrom;
use std::{fs, str::FromStr};

use chrono::prelude::*;
use rusqlite::{Connection, NO_PARAMS};
use serde::Deserialize;
use serde_hex::{SerHex, StrictPfx};
use url::Url;

#[derive(Deserialize, Clone)]
pub struct Config {
    pub database_path: String,

    #[serde(rename = "api_key")]
    _api_key: String,
    #[serde(rename = "api_secret")]
    _api_secret: String,
    #[serde(rename = "access_token")]
    _access_token: String,
    #[serde(rename = "access_token_secret")]
    _access_token_secret: String,

    pub azure_key: String,
    pub deepl_key: String,
    pub bearer_token: String,
    pub discord_token: String,

    pub twitter_channel: u64,
    pub live_notif_channel: u64,
    pub schedule_channel: u64,
    pub birthday_notif_channel: u64,

    #[serde(skip)]
    pub users: Vec<User>,
}

impl Config {
    pub fn load_config(path: &str) -> Self {
        let config_json = fs::read_to_string(path).expect("Something went wrong reading the file.");
        let mut config: Config =
            serde_json::from_str(&config_json).expect("Couldn't parse config.");

        config.load_database().expect("Couldn't load database!");
        return config;
    }

    fn load_database(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let db = Connection::open(&self.database_path)?;
        let mut user_stmt = db.prepare("SELECT name, display_name, icon_url, channel_id, birthday_day, birthday_month, 
                                                timezone, twitter_name, twitter_id, colour, discord_role, schedule_keyword
                                                FROM users").unwrap();

        self.users = user_stmt
            .query_map(NO_PARAMS, |row| {
                Ok(User {
                    name: row.get("name")?,
                    display_name: row.get("display_name")?,
                    icon: row.get("icon_url")?,
                    channel: row.get("channel_id")?,
                    birthday: (row.get("birthday_day")?, row.get("birthday_month")?),
                    timezone: chrono_tz::Tz::from_str(&row.get::<&str, String>("timezone")?)
                        .unwrap(),
                    twitter_handle: row.get("twitter_name")?,
                    twitter_id: u64::try_from(row.get::<&str, i64>("twitter_id")?).unwrap(),
                    colour: u32::from_str_radix(&row.get::<&str, String>("colour")?, 16).unwrap(),
                    discord_role: u64::try_from(row.get::<&str, i64>("discord_role")?).unwrap(),
                    schedule_keyword: row.get("schedule_keyword").ok(),
                })
            })
            .unwrap()
            .map(|u| u.unwrap())
            .collect::<Vec<_>>();

        Ok(())
    }
}

#[derive(Deserialize, Clone, Debug)]
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
    pub schedule_keyword: Option<String>,

    #[serde(with = "SerHex::<StrictPfx>")]
    pub colour: u32,
    pub discord_role: u64,
}

impl User {
    pub fn get_next_birthday(&self) -> DateTime<Utc> {
        let now = Utc::now();
        let mut year = now.year();

        let (day, month) = self.birthday;

        if month < now.month() || (month == now.month() && day <= now.day()) {
            year += 1;
        }

        self.timezone
            .ymd(year, month, day)
            .and_hms(12, 0, 0)
            .with_timezone(&Utc)
    }
}
