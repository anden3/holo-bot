use std::collections::{HashMap, HashSet};
use std::{fs, str::FromStr};

use anyhow::{anyhow, Context};
use chrono::prelude::*;
use log::error;
use rusqlite::{types::FromSqlError, Connection};
use serde::Deserialize;
use serde_hex::{SerHex, StrictPfx};
use serenity::{
    model::id::{ChannelId, EmojiId},
    prelude::TypeMapKey,
};
use strum_macros::{EnumString, ToString};
use url::Url;

use super::here;

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

    #[serde(default = "bool::default")]
    pub development: bool,
    #[serde(default = "HashSet::new")]
    pub blocked_servers: HashSet<u64>,

    pub twitter_feeds: HashMap<HoloBranch, HashMap<HoloGeneration, u64>>,

    #[serde(skip)]
    pub users: Vec<User>,
}

impl Config {
    pub fn load_config(path: &str) -> anyhow::Result<Self> {
        let config_json = fs::read_to_string(path).context(here!())?;
        let mut config: Self = serde_json::from_str(&config_json).context(here!())?;

        config.get_users()?;
        Ok(config)
    }

    pub fn get_database_handle(&self) -> anyhow::Result<Connection> {
        Connection::open(&self.database_path).context(here!())
    }

    pub fn get_emoji_usage(database_handle: &Connection) -> anyhow::Result<HashMap<EmojiId, u64>> {
        database_handle.execute("CREATE TABLE IF NOT EXISTS emoji_usage (emoji_id INTEGER PRIMARY KEY, count INTEGER NOT NULL)", []).context(here!())?;

        let mut stmt = database_handle
            .prepare("SELECT emoji_id, count FROM emoji_usage")
            .context(here!())?;

        let result = stmt
            .query_and_then::<_, anyhow::Error, _, _>([], |row| {
                Ok((
                    EmojiId(row.get("emoji_id").context(here!())?),
                    row.get("count").context(here!())?,
                ))
            })?
            .map(std::result::Result::unwrap);

        Ok(result.into_iter().collect::<HashMap<_, _>>())
    }

    pub fn save_emoji_usage(
        database_handle: &Connection,
        emoji_usage: &HashMap<EmojiId, u64>,
    ) -> anyhow::Result<()> {
        database_handle.execute("CREATE TABLE IF NOT EXISTS emoji_usage (emoji_id INTEGER PRIMARY KEY, count INTEGER NOT NULL)", []).context(here!())?;

        let mut stmt = database_handle
            .prepare_cached("INSERT INTO emoji_usage (emoji_id, count) VALUES (?, ?)")?;

        let tx = database_handle.unchecked_transaction()?;

        for (emoji, count) in emoji_usage {
            stmt.execute([emoji.as_u64(), count])?;
        }

        tx.commit()?;
        Ok(())
    }

    fn get_users(&mut self) -> anyhow::Result<()> {
        let db = Connection::open(&self.database_path).context(here!())?;
        let mut user_stmt = db.prepare("SELECT name, display_name, emoji, branch, generation, icon_url, channel_id, birthday_day, birthday_month, 
                                                timezone, twitter_name, twitter_id, colour, discord_role, schedule_keyword
                                                FROM users").context(here!())?;

        self.users = user_stmt
            .query_and_then::<_, anyhow::Error, _, _>([], |row| {
                let timezone =
                    chrono_tz::Tz::from_str(&row.get::<&str, String>("timezone").context(here!())?)
                        .map_err(|e| anyhow!(e))
                        .context(here!())?;
                let colour =
                    u32::from_str_radix(&row.get::<&str, String>("colour").context(here!())?, 16)
                        .context(here!())?;

                Ok(User {
                    name: row.get("name").context(here!())?,
                    display_name: row.get("display_name").context(here!())?,
                    emoji: row.get("emoji").context(here!())?,
                    branch: row.get("branch").context(here!())?,
                    generation: row.get("generation").context(here!())?,
                    icon: row.get("icon_url").context(here!())?,
                    channel: row.get("channel_id").context(here!())?,
                    birthday: (
                        row.get("birthday_day").context(here!())?,
                        row.get("birthday_month").context(here!())?,
                    ),
                    timezone,
                    twitter_handle: row.get("twitter_name").context(here!())?,
                    twitter_id: row.get("twitter_id").context(here!())?,
                    colour,
                    discord_role: row.get("discord_role").context(here!())?,
                    schedule_keyword: row.get("schedule_keyword").context(here!())?,
                })
            })?
            .map(std::result::Result::unwrap)
            .collect::<Vec<_>>();

        Ok(())
    }
}

impl TypeMapKey for Config {
    type Value = Self;
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
    #[must_use]
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

    #[must_use]
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
    fn eq(&self, other: &Self) -> bool {
        self.twitter_id == other.twitter_id
    }
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Deserialize, Debug, Hash, Eq, PartialEq, Copy, Clone, EnumString, ToString)]
pub enum HoloBranch {
    HoloJP,
    HoloID,
    HoloEN,
    HolostarsJP,
}

impl rusqlite::types::FromSql for HoloBranch {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let str = value.as_str()?;

        Self::from_str(str).map_err(|e| {
            error!("{}: '{}'", e, str);
            FromSqlError::InvalidType
        })
    }
}

#[allow(clippy::upper_case_acronyms)]
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
        let str = value.as_str()?;

        Self::from_str(str).map_err(|e| {
            error!("{}: '{}'", e, str);
            FromSqlError::InvalidType
        })
    }
}
