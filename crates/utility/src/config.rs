use std::collections::{HashMap, HashSet};
use std::{fs, str::FromStr};

use anyhow::{anyhow, Context};
use chrono::prelude::*;
use regex::Regex;
use rusqlite::{types::FromSqlError, Connection};
use serde::{Deserialize, Serialize};
use serde_hex::{SerHex, StrictPfx};
use serenity::{
    builder::CreateEmbed,
    model::id::{ChannelId, EmojiId},
    prelude::TypeMapKey,
};
use strum_macros::{EnumIter, EnumString, ToString};
use tracing::error;
use url::Url;

use super::here;
use crate::regex;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub database_path: String,

    pub azure_key: String,
    pub deepl_key: String,
    pub openai_token: String,
    pub twitter_token: String,
    pub discord_token: String,
    pub imgflip_user: String,
    pub imgflip_pass: String,

    pub live_notif_channel: u64,
    pub schedule_channel: u64,
    pub birthday_notif_channel: u64,

    pub stream_chat_category: u64,
    pub stream_chat_logs: u64,

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

    pub fn get_emoji_usage(
        database_handle: &Connection,
    ) -> anyhow::Result<HashMap<EmojiId, EmojiStats>> {
        database_handle.execute("CREATE TABLE IF NOT EXISTS emoji_usage (emoji_id INTEGER PRIMARY KEY, text_count INTEGER NOT NULL, reaction_count INTEGER NOT NULL)", []).context(here!())?;

        let mut stmt = database_handle
            .prepare("SELECT emoji_id, text_count, reaction_count FROM emoji_usage")
            .context(here!())?;

        let result = stmt
            .query_and_then::<_, anyhow::Error, _, _>([], |row| {
                Ok((
                    EmojiId(row.get("emoji_id").context(here!())?),
                    EmojiStats {
                        text_count: row.get("text_count").context(here!())?,
                        reaction_count: row.get("reaction_count").context(here!())?,
                    },
                ))
            })?
            .map(std::result::Result::unwrap);

        Ok(result.into_iter().collect::<HashMap<_, _>>())
    }

    pub fn save_emoji_usage(
        database_handle: &Connection,
        emoji_usage: &HashMap<EmojiId, EmojiStats>,
    ) -> anyhow::Result<()> {
        database_handle.execute("CREATE TABLE IF NOT EXISTS emoji_usage (emoji_id INTEGER PRIMARY KEY, text_count INTEGER NOT NULL, reaction_count INTEGER NOT NULL)", []).context(here!())?;

        let mut stmt = database_handle.prepare_cached(
            "INSERT OR REPLACE INTO emoji_usage (emoji_id, text_count, reaction_count) VALUES (?, ?, ?)",
        )?;

        let tx = database_handle.unchecked_transaction()?;

        for (emoji, count) in emoji_usage {
            stmt.execute([emoji.as_u64(), &count.text_count, &count.reaction_count])?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn get_quotes(database_handle: &Connection) -> anyhow::Result<Vec<Quote>> {
        database_handle
            .execute(
                "CREATE TABLE IF NOT EXISTS Quotes (lines BLOB NOT NULL)",
                [],
            )
            .context(here!())?;

        let mut stmt = database_handle
            .prepare("SELECT lines FROM Quotes")
            .context(here!())?;

        let result = stmt
            .query_and_then::<_, anyhow::Error, _, _>([], |row| {
                let quote_lines: Vec<QuoteLine> = serde_json::from_value(row.get(0)?)?;
                let quote = Quote { lines: quote_lines };

                Ok(quote)
            })?
            .map(std::result::Result::unwrap)
            .collect();

        Ok(result)
    }

    pub fn save_quotes(database_handle: &Connection, quotes: &[Quote]) -> anyhow::Result<()> {
        database_handle
            .execute(
                "CREATE TABLE IF NOT EXISTS Quotes (lines BLOB NOT NULL)",
                [],
            )
            .context(here!())?;

        let mut stmt =
            database_handle.prepare_cached("INSERT OR REPLACE INTO Quotes (lines) VALUES (?)")?;

        let tx = database_handle.unchecked_transaction()?;

        for quote in quotes {
            stmt.execute([serde_json::to_value(quote.lines.clone())?])?;
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

trait UserCollection {
    fn find_by_name(&self, name: &str) -> Option<&User>;
}

impl UserCollection for &[User] {
    fn find_by_name(&self, name: &str) -> Option<&User> {
        self.iter().find(|u| {
            u.display_name
                .to_lowercase()
                .contains(&name.trim().to_lowercase())
        })
    }
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Deserialize, Debug, Hash, Eq, PartialEq, Copy, Clone, EnumString, ToString, EnumIter)]
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

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct EmojiStats {
    pub text_count: u64,
    pub reaction_count: u64,
}

impl std::ops::AddAssign for EmojiStats {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs
    }
}

impl std::ops::Add for EmojiStats {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            text_count: self.text_count + rhs.text_count,
            reaction_count: self.reaction_count + rhs.reaction_count,
        }
    }
}

impl PartialOrd for EmojiStats {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for EmojiStats {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.total().cmp(&other.total())
    }
}

impl EmojiStats {
    pub fn total(&self) -> u64 {
        self.text_count + self.reaction_count
    }
}

#[derive(Debug, Clone, Default)]
pub struct Quote {
    pub lines: Vec<QuoteLine>,
}

impl Quote {
    pub fn from_message(msg: &str, users: &[User]) -> anyhow::Result<Self> {
        let quote_rgx: &'static Regex = regex!(r#"^\s*(.+?): ?(.+?)\s*$"#);

        let mut lines = Vec::new();

        for line in msg.split('|') {
            let capture = quote_rgx
                .captures(line)
                .ok_or_else(|| anyhow!("Invalid quote string."))?;

            let name = capture
                .get(1)
                .ok_or_else(|| anyhow!("Could not find name!"))?
                .as_str()
                .trim()
                .to_lowercase();

            let text = capture
                .get(2)
                .ok_or_else(|| anyhow!("Invalid quote!"))?
                .as_str()
                .trim();

            let name = &users
                .find_by_name(&name)
                .ok_or_else(|| anyhow!("No talent found with the name {}!", name))?
                .name;

            lines.push(QuoteLine {
                user: name.to_owned(),
                line: text.to_string(),
            });
        }

        Ok(Quote { lines })
    }

    pub fn load_users<'a>(&self, users: &'a [User]) -> anyhow::Result<Vec<(&'a User, &String)>> {
        let lines: anyhow::Result<Vec<_>> = self
            .lines
            .iter()
            .map(|l| {
                let user = users
                    .iter()
                    .find(|u| u.name == l.user)
                    .ok_or_else(|| anyhow!("User {} not found!", l.user))
                    .context(here!());

                match user {
                    Ok(u) => Ok((u, &l.line)),
                    Err(e) => Err(e),
                }
            })
            .collect();

        lines
    }

    pub fn as_embed(&self, users: &[User]) -> anyhow::Result<CreateEmbed> {
        let fields = self.load_users(users)?;
        let mut embed = CreateEmbed::default();

        embed.fields(
            fields
                .into_iter()
                .map(|(u, l)| (u.display_name.clone(), l.clone(), false)),
        );

        Ok(embed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteLine {
    pub user: String,
    pub line: String,
}
