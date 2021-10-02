use std::{
    collections::{HashMap, HashSet},
    fs,
    str::FromStr,
};

use anyhow::{anyhow, Context};
use chrono::prelude::*;
use chrono_tz::Tz;
use regex::Regex;
use rusqlite::{
    types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, Value, ValueRef},
    Connection, ToSql,
};
use serde::{Deserialize, Serialize};
use serde_hex::{CompactPfx, SerHex};
use serde_with::{serde_as, DisplayFromStr};
use serenity::{
    builder::CreateEmbed,
    model::id::{ChannelId, UserId},
    prelude::TypeMapKey,
};
use strum_macros::{EnumIter, EnumString, ToString};
use url::Url;

use crate::{here, regex};

#[derive(Debug, Deserialize)]
struct TalentFile {
    talents: Vec<Talent>,
}

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
    pub holodex_key: String,

    pub live_notif_channel: u64,
    pub schedule_channel: u64,
    pub birthday_notif_channel: u64,

    pub stream_chat_category: u64,
    pub stream_chat_logs: u64,

    #[serde(default = "bool::default")]
    pub development: bool,
    #[serde(default = "HashSet::new")]
    pub blocked_servers: HashSet<u64>,

    pub branch_channels: HashMap<HoloBranch, u64>,
    pub twitter_feeds: HashMap<HoloBranch, HashMap<HoloGeneration, u64>>,

    #[serde(skip)]
    pub talents: Vec<Talent>,
}

impl Config {
    pub fn load_config(folder: &str) -> anyhow::Result<Self> {
        let config_json =
            fs::read_to_string(format!("{}/holobot.json", folder)).context(here!())?;
        let mut config: Self = serde_json::from_str(&config_json).context(here!())?;

        let db_handle = Connection::open(&config.database_path).context(here!())?;

        Self::initialize_tables(&db_handle)?;

        let talents = fs::read_to_string(format!("{}/talents.toml", folder)).context(here!())?;
        let talents_toml: TalentFile = toml::from_str(&talents).context(here!())?;
        config.talents = talents_toml.talents;

        Ok(config)
    }

    fn initialize_tables(handle: &Connection) -> anyhow::Result<()> {
        handle
            .execute(
                "CREATE TABLE IF NOT EXISTS emoji_usage (emoji_id INTEGER PRIMARY KEY, text_count INTEGER NOT NULL, reaction_count INTEGER NOT NULL)", 
                []
            )
            .context(here!())?;

        handle
            .execute(
                "CREATE TABLE IF NOT EXISTS Quotes (quote BLOB NOT NULL)",
                [],
            )
            .context(here!())?;

        handle
            .execute(
                "CREATE TABLE IF NOT EXISTS Reminders (reminder BLOB NOT NULL)",
                [],
            )
            .context(here!())?;

        handle
            .execute(
                "CREATE TABLE IF NOT EXISTS NotifiedCache (stream_id TEXT NOT NULL)",
                [],
            )
            .context(here!())?;

        Ok(())
    }

    pub fn get_database_handle(&self) -> anyhow::Result<Connection> {
        Connection::open(&self.database_path).context(here!())
    }

    pub fn open_database(path: &str) -> anyhow::Result<Connection> {
        Connection::open(path).context(here!())
    }
}

impl TypeMapKey for Config {
    type Value = Self;
}

pub trait SaveToDatabase {
    fn save_to_database(&self, handle: &Connection) -> anyhow::Result<()>;
}

pub trait LoadFromDatabase {
    type Item;
    type ItemContainer: IntoIterator<Item = Self::Item>;

    fn load_from_database(handle: &Connection) -> anyhow::Result<Self::ItemContainer>
    where
        Self::Item: Sized;
}

#[derive(Debug, Clone, Deserialize)]
pub struct Birthday {
    pub day: u8,
    pub month: u8,
    pub year: Option<i16>,
}

#[serde_as]
#[derive(Debug, Clone, Deserialize)]
pub struct Talent {
    pub name: String,
    pub english_name: String,
    pub emoji: String,
    pub icon: Url,

    pub branch: HoloBranch,
    pub generation: HoloGeneration,

    pub birthday: Birthday,
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub timezone: Option<chrono_tz::Tz>,

    pub youtube_ch_id: Option<String>,
    pub twitter_handle: Option<String>,
    pub twitter_id: Option<u64>,
    pub schedule_keyword: Option<String>,

    #[serde(with = "SerHex::<CompactPfx>")]
    #[serde(default)]
    pub colour: u32,
    pub discord_role: Option<u64>,
}

impl Talent {
    #[must_use]
    pub fn get_next_birthday(&self) -> DateTime<Utc> {
        let now = Utc::now();
        let Birthday {
            day,
            month,
            year: _year,
        } = self.birthday;
        let current_year = now.year();

        let birthday = self
            .timezone
            .unwrap_or(Tz::UTC)
            .ymd(current_year, month as _, day as _)
            .and_hms(0, 0, 0)
            .with_timezone(&Utc);

        if birthday < now {
            birthday.with_year(current_year + 1).unwrap_or(birthday)
        } else {
            birthday
        }
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

impl std::fmt::Display for Talent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.english_name)
    }
}

impl PartialEq for Talent {
    fn eq(&self, other: &Self) -> bool {
        self.twitter_id == other.twitter_id
    }
}

pub trait UserCollection {
    fn find_by_name(&self, name: &str) -> Option<&Talent>;
}

impl UserCollection for &[Talent] {
    fn find_by_name(&self, name: &str) -> Option<&Talent> {
        self.iter().find(|u| {
            u.english_name
                .to_lowercase()
                .contains(&name.trim().to_lowercase())
        })
    }
}

impl UserCollection for Vec<Talent> {
    fn find_by_name(&self, name: &str) -> Option<&Talent> {
        self.iter().find(|u| {
            u.english_name
                .to_lowercase()
                .contains(&name.trim().to_lowercase())
        })
    }
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Deserialize, Debug, Hash, Eq, PartialEq, Copy, Clone, EnumString, ToString, EnumIter)]
#[non_exhaustive]
pub enum HoloBranch {
    HoloJP,
    HoloID,
    HoloEN,
    HolostarsJP,
}

impl FromSql for HoloBranch {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        Self::from_str(value.as_str()?).map_err(|e| FromSqlError::Other(Box::new(e)))
    }
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Deserialize, Debug, Hash, Eq, PartialEq, Copy, Clone, EnumString, ToString)]
#[non_exhaustive]
pub enum HoloGeneration {
    Staff,
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
    ProjectHope,
    Council,
}

impl FromSql for HoloGeneration {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        Self::from_str(value.as_str()?).map_err(|e| FromSqlError::Other(Box::new(e)))
    }
}

#[derive(Debug, Copy, Clone)]
pub enum EmojiUsageSource {
    InText,
    AsReaction,
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
    pub fn add(&mut self, source: EmojiUsageSource) {
        match source {
            EmojiUsageSource::InText => self.text_count += 1,
            EmojiUsageSource::AsReaction => self.reaction_count += 1,
        }
    }

    pub fn total(&self) -> u64 {
        self.text_count + self.reaction_count
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Quote {
    #[serde(default = "Vec::new")]
    pub lines: Vec<QuoteLine>,
}

impl Quote {
    pub fn from_message(msg: &str, users: &[Talent]) -> anyhow::Result<Self> {
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

    pub fn load_users<'a>(
        &self,
        users: &'a [Talent],
    ) -> anyhow::Result<Vec<(&'a Talent, &String)>> {
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

    pub fn as_embed(&self, users: &[Talent]) -> anyhow::Result<CreateEmbed> {
        let fields = self.load_users(users)?;
        let mut embed = CreateEmbed::default();

        embed.fields(
            fields
                .into_iter()
                .map(|(u, l)| (u.english_name.clone(), l.clone(), false)),
        );

        Ok(embed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteLine {
    pub user: String,
    pub line: String,
}

impl FromSql for Quote {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        bincode::deserialize(value.as_blob()?).map_err(|e| FromSqlError::Other(e))
    }
}

impl ToSql for Quote {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Owned(Value::Blob(
            bincode::serialize(self).map_err(|e| rusqlite::Error::ToSqlConversionFailure(e))?,
        )))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq)]
pub struct Reminder {
    pub id: u64,
    pub message: String,
    pub time: DateTime<Utc>,
    pub subscribers: Vec<ReminderSubscriber>,
}

impl PartialEq for Reminder {
    fn eq(&self, other: &Self) -> bool {
        self.id.eq(&other.id)
    }
}

impl PartialOrd for Reminder {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.time.partial_cmp(&other.time)
    }
}

impl Ord for Reminder {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.time.cmp(&other.time)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReminderSubscriber {
    pub user: UserId,
    pub location: ReminderLocation,
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    EnumIter,
    ToString,
    EnumString,
)]
pub enum ReminderLocation {
    DM,
    Channel(ChannelId),
}

impl FromSql for Reminder {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        bincode::deserialize(value.as_blob()?).map_err(|e| FromSqlError::Other(e))
    }
}

impl ToSql for Reminder {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Owned(Value::Blob(
            bincode::serialize(self).map_err(|e| rusqlite::Error::ToSqlConversionFailure(e))?,
        )))
    }
}

impl SaveToDatabase for &[Reminder] {
    fn save_to_database(&self, handle: &Connection) -> anyhow::Result<()> {
        let mut stmt =
            handle.prepare_cached("INSERT OR REPLACE INTO Reminders (reminder) VALUES (?)")?;

        let tx = handle.unchecked_transaction()?;

        for reminder in self.iter() {
            stmt.execute([reminder])?;
        }

        tx.commit()?;
        Ok(())
    }
}

impl LoadFromDatabase for Reminder {
    type Item = Reminder;
    type ItemContainer = Vec<Self::Item>;

    fn load_from_database(handle: &Connection) -> anyhow::Result<Self::ItemContainer> {
        let mut stmt = handle
            .prepare("SELECT reminder FROM Reminders")
            .context(here!())?;

        let results = stmt.query_and_then([], |row| -> anyhow::Result<Self::Item> {
            row.get(0).map_err(|e| anyhow!(e))
        })?;

        results.collect()
    }
}

pub enum EntryEvent<K, V> {
    Added { key: K, value: V },
    Updated { key: K, value: V },
    Removed { key: K },
}

impl<T> SaveToDatabase for tokio::sync::MutexGuard<'_, T>
where
    T: SaveToDatabase,
{
    fn save_to_database(&self, handle: &Connection) -> anyhow::Result<()> {
        use std::ops::Deref;
        self.deref().save_to_database(handle)
    }
}
