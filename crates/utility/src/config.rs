use std::{
    collections::{HashMap, HashSet},
    fs,
    str::FromStr,
    sync::Arc,
};

use anyhow::{anyhow, Context};
use chrono::{prelude::*, Duration};
use chrono_tz::Tz;
use regex::Regex;
use rusqlite::{
    types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, Value, ValueRef},
    Connection, ToSql,
};
use serde::{Deserialize, Serialize};
use serde_hex::{CompactPfx, SerHex};
use serde_with::{serde_as, DeserializeFromStr, DisplayFromStr, DurationSeconds, SerializeDisplay};
use serenity::{
    builder::CreateEmbed,
    model::id::{ChannelId, EmojiId, GuildId, RoleId, UserId},
    prelude::TypeMapKey,
};
use strum_macros::{Display, EnumIter, EnumString, ToString};

use crate::{
    functions::{default_true, is_default},
    here, regex,
    types::TranslatorType,
};

#[derive(Debug, Deserialize)]
struct TalentFile {
    talents: Vec<Talent>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Config {
    pub discord_token: String,
    pub blocked: BlockedEntities,
    #[serde(skip_serializing_if = "is_default")]
    pub database: Database,

    #[serde(default)]
    pub stream_tracking: StreamTrackingConfig,

    #[serde(default)]
    pub music_bot: MusicBotConfig,

    #[serde(default)]
    pub birthday_alerts: BirthdayAlertsConfig,

    #[serde(default)]
    pub emoji_tracking: EmojiTrackingConfig,

    #[serde(default)]
    pub meme_creation: MemeCreationConfig,

    #[serde(default)]
    pub ai_chatbot: AiChatbotConfig,

    #[serde(default)]
    pub reminders: ReminderConfig,

    #[serde(default)]
    pub quotes: QuoteConfig,

    #[serde(default)]
    pub twitter: TwitterConfig,

    #[serde(default)]
    pub react_temp_mute: ReactTempMuteConfig,

    #[serde(skip)]
    pub talents: Vec<Talent>,
}

impl Config {
    pub fn load(folder: &str) -> anyhow::Result<Arc<Self>> {
        let config_toml = fs::read_to_string(format!("{}/config.toml", folder)).context(here!())?;
        let mut config: Config = toml::from_str(&config_toml).context(here!())?;

        let handle = config.database.get_handle()?;
        Database::initialize_tables(&handle)?;

        let talents = fs::read_to_string(format!("{}/talents.toml", folder)).context(here!())?;
        let talents_toml: TalentFile = toml::from_str(&talents).context(here!())?;
        config.talents = talents_toml.talents;

        Ok(Arc::new(config))
    }
}

impl TypeMapKey for Config {
    type Value = Self;
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct BlockedEntities {
    #[serde(default)]
    pub users: HashSet<UserId>,
    #[serde(default)]
    pub servers: HashSet<GuildId>,
    #[serde(default)]
    pub channels: HashSet<ChannelId>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "backend", content = "parameters")]
pub enum Database {
    SQLite { path: String },
}

impl Default for Database {
    fn default() -> Self {
        Self::SQLite {
            path: "".to_string(),
        }
    }
}

impl Database {
    pub fn get_handle(&self) -> anyhow::Result<DatabaseHandle> {
        match self {
            Database::SQLite { path } => {
                let conn = Connection::open(path).context(here!())?;
                Ok(DatabaseHandle::SQLite(conn))
            }
        }
    }

    pub fn initialize_tables(handle: &DatabaseHandle) -> anyhow::Result<()> {
        match handle {
            DatabaseHandle::SQLite(h) => {
                h.execute(
                    "CREATE TABLE IF NOT EXISTS emoji_usage (emoji_id INTEGER PRIMARY KEY, text_count INTEGER NOT NULL, reaction_count INTEGER NOT NULL)", 
                    []
                )
                .context(here!())?;

                h.execute(
                    "CREATE TABLE IF NOT EXISTS Quotes (quote BLOB NOT NULL)",
                    [],
                )
                .context(here!())?;

                h.execute(
                    "CREATE TABLE IF NOT EXISTS Reminders (reminder BLOB NOT NULL)",
                    [],
                )
                .context(here!())?;

                h.execute(
                    "CREATE TABLE IF NOT EXISTS NotifiedCache (stream_id TEXT NOT NULL)",
                    [],
                )
                .context(here!())?;
            }
        }

        Ok(())
    }
}

#[derive(Debug)]
pub enum DatabaseHandle {
    SQLite(Connection),
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct StreamTrackingConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub holodex_token: String,

    #[serde(default)]
    pub alerts: StreamAlertsConfig,

    #[serde(default)]
    pub chat: StreamChatConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct StreamAlertsConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub channel: ChannelId,
}

#[serde_as]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct StreamChatConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub category: ChannelId,

    #[serde(default)]
    pub logging_channel: Option<ChannelId>,

    #[serde(default)]
    #[serde_as(as = "HashMap<DisplayFromStr, _>")]
    pub post_stream_discussion: HashMap<HoloBranch, ChannelId>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct MusicBotConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub channel: ChannelId,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct BirthdayAlertsConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub channel: ChannelId,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct EmojiTrackingConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct MemeCreationConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub imgflip_user: String,
    pub imgflip_pass: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct AiChatbotConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub openai_token: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ReminderConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct QuoteConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct TwitterConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub token: String,

    #[serde(default)]
    pub schedule_updates: ScheduleUpdateConfig,

    #[serde(default)]
    pub feeds: HashMap<HoloBranch, HashMap<HoloGeneration, ChannelId>>,

    #[serde(default)]
    pub feed_translation: HashMap<TranslatorType, TranslatorConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ScheduleUpdateConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub channel: ChannelId,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TranslatorConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub token: String,
    #[serde(default)]
    pub languages: Vec<String>,
}

#[serde_as]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReactTempMuteConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub mute_role: RoleId,
    pub required_reaction_count: usize,
    pub excessive_mute_threshold: usize,
    #[serde_as(as = "DurationSeconds<i64>")]
    pub mute_duration: Duration,
    #[serde_as(as = "DurationSeconds<i64>")]
    pub eligibility_duration: Duration,
    pub reactions: HashSet<EmojiId>,

    #[serde(default)]
    pub logging_channel: Option<ChannelId>,
}

impl Default for ReactTempMuteConfig {
    fn default() -> Self {
        ReactTempMuteConfig {
            enabled: false,
            mute_role: RoleId::default(),
            required_reaction_count: 3,
            excessive_mute_threshold: 3,
            mute_duration: Duration::minutes(5),
            eligibility_duration: Duration::minutes(5),
            reactions: HashSet::new(),
            logging_channel: None,
        }
    }
}

pub trait SaveToDatabase {
    fn save_to_database(&self, handle: &DatabaseHandle) -> anyhow::Result<()>;
}

pub trait LoadFromDatabase {
    type Item;
    type ItemContainer: IntoIterator<Item = Self::Item>;

    fn load_from_database(handle: &DatabaseHandle) -> anyhow::Result<Self::ItemContainer>
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
    pub icon: String,

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
    pub discord_role: Option<RoleId>,
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
        *config
            .twitter
            .feeds
            .get(&self.branch)
            .unwrap()
            .get(&self.generation)
            .unwrap()
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
#[derive(
    Debug,
    Hash,
    Eq,
    PartialEq,
    Copy,
    Clone,
    Display,
    EnumString,
    EnumIter,
    SerializeDisplay,
    DeserializeFromStr,
)]
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
#[derive(
    Debug,
    Hash,
    Eq,
    PartialEq,
    Copy,
    Clone,
    EnumString,
    Display,
    SerializeDisplay,
    DeserializeFromStr,
)]
#[non_exhaustive]
pub enum HoloGeneration {
    Staff,
    #[strum(to_string = "0th")]
    _0th,
    #[strum(to_string = "1st")]
    _1st,
    #[strum(to_string = "2nd")]
    _2nd,
    #[strum(to_string = "3rd")]
    _3rd,
    #[strum(to_string = "4th")]
    _4th,
    #[strum(to_string = "5th")]
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
    fn save_to_database(&self, handle: &DatabaseHandle) -> anyhow::Result<()> {
        match handle {
            DatabaseHandle::SQLite(h) => {
                let mut stmt =
                    h.prepare_cached("INSERT OR REPLACE INTO Reminders (reminder) VALUES (?)")?;

                let tx = h.unchecked_transaction()?;

                for reminder in self.iter() {
                    stmt.execute([reminder])?;
                }

                tx.commit()?;
            }
        }

        Ok(())
    }
}

impl LoadFromDatabase for Reminder {
    type Item = Reminder;
    type ItemContainer = Vec<Self::Item>;

    fn load_from_database(handle: &DatabaseHandle) -> anyhow::Result<Self::ItemContainer> {
        match handle {
            DatabaseHandle::SQLite(h) => {
                let mut stmt = h
                    .prepare("SELECT reminder FROM Reminders")
                    .context(here!())?;

                let results = stmt.query_and_then([], |row| -> anyhow::Result<Self::Item> {
                    row.get(0).map_err(|e| anyhow!(e))
                })?;

                results.collect()
            }
        }
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
    fn save_to_database(&self, handle: &DatabaseHandle) -> anyhow::Result<()> {
        use std::ops::Deref;
        self.deref().save_to_database(handle)
    }
}
