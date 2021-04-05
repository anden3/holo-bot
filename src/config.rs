use std::collections::HashMap;
use std::{fs, str::FromStr};

use chrono::prelude::*;
use log::error;
use rusqlite::{types::FromSqlError, Connection};
use serde::Deserialize;
use serde_hex::{SerHex, StrictPfx};
use serenity::model::id::ChannelId;
use strum_macros::{EnumString, ToString};
use url::Url;

#[derive(Deserialize, Clone)]
pub struct Config {
    pub database_path: String,

    pub azure_key: String,
    pub deepl_key: String,
    pub twitter_token: String,
    pub discord_token: String,
    pub imgflip_user: String,
    pub imgflip_pass: String,

    pub live_notif_channel: u64,
    pub schedule_channel: u64,
    pub birthday_notif_channel: u64,

    pub twitter_feeds: HashMap<HoloBranch, HashMap<HoloGeneration, u64>>,

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
        let mut user_stmt = db.prepare("SELECT name, display_name, emoji, branch, generation, icon_url, channel_id, birthday_day, birthday_month, 
                                                timezone, twitter_name, twitter_id, colour, discord_role, schedule_keyword
                                                FROM users").unwrap();

        self.users = user_stmt
            .query_map([], |row| {
                Ok(User {
                    name: row.get("name")?,
                    display_name: row.get("display_name")?,
                    emoji: row.get("emoji")?,
                    branch: row.get("branch")?,
                    generation: row.get("generation")?,
                    icon: row.get("icon_url")?,
                    channel: row.get("channel_id")?,
                    birthday: (row.get("birthday_day")?, row.get("birthday_month")?),
                    timezone: chrono_tz::Tz::from_str(&row.get::<&str, String>("timezone")?)
                        .unwrap(),
                    twitter_handle: row.get("twitter_name")?,
                    twitter_id: row.get("twitter_id")?,
                    colour: u32::from_str_radix(&row.get::<&str, String>("colour")?, 16).unwrap(),
                    discord_role: row.get("discord_role")?,
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
    pub emoji: String,

    pub branch: HoloBranch,
    pub generation: HoloGeneration,

    pub icon: Url,
    pub channel: String,

    pub birthday: (u32, u32),
    #[serde(with = "crate::utility::serializers::timezone")]
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

    pub fn get_twitter_channel(&self, config: &Config) -> ChannelId {
        ChannelId(
            *config
                .twitter_feeds
                .get(&self.branch)
                .unwrap()
                .get(&self.generation)
                .unwrap(),
        )
    }
}

impl std::fmt::Display for User {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name)
    }
}

impl PartialEq for User {
    fn ne(&self, other: &Self) -> bool {
        self.twitter_id != other.twitter_id
    }

    fn eq(&self, other: &Self) -> bool {
        self.twitter_id == other.twitter_id
    }
}

#[derive(Deserialize, Debug, Hash, Eq, PartialEq, Copy, Clone, EnumString, ToString)]
pub enum HoloBranch {
    HoloJP,
    HoloID,
    HoloEN,
    HolostarsJP,
}

impl rusqlite::types::FromSql for HoloBranch {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        HoloBranch::from_str(value.as_str().unwrap()).map_err(|e| {
            error!("{}: '{}'", e, value.as_str().unwrap());
            FromSqlError::InvalidType
        })
    }
}

#[derive(Deserialize, Debug, Hash, Eq, PartialEq, Copy, Clone, EnumString, ToString)]
pub enum HoloGeneration {
    #[serde(rename = "0th")]
    #[strum(serialize = "0th")]
    _0th,
    #[serde(rename = "1st")]
    #[strum(serialize = "1st")]
    _1st,
    #[serde(rename = "2nd")]
    #[strum(serialize = "2nd")]
    _2nd,
    #[serde(rename = "3rd")]
    #[strum(serialize = "3rd")]
    _3rd,
    #[serde(rename = "4th")]
    #[strum(serialize = "4th")]
    _4th,
    #[serde(rename = "5th")]
    #[strum(serialize = "5th")]
    _5th,
    GAMERS,
}

impl rusqlite::types::FromSql for HoloGeneration {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        HoloGeneration::from_str(value.as_str().unwrap()).map_err(|e| {
            error!("{}: '{}'", e, value.as_str().unwrap());
            FromSqlError::InvalidType
        })
    }
}
