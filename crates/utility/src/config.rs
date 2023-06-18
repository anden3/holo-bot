mod functions;
mod types;

use std::{fmt::Display, path::Path, str::FromStr, sync::Arc};

use anyhow::Context;
use chrono::prelude::*;
use chrono_tz::Tz;
// use music_queue::EnqueuedItem;
use rusqlite::{
    types::{FromSql, FromSqlError, FromSqlResult, ValueRef},
    ToSql,
};
use serde::{Deserialize, Serialize};
use serde_hex::{CompactPfx, SerHex};
use serde_with::{serde_as, DeserializeFromStr, DisplayFromStr, SerializeDisplay};
use serenity::{
    model::id::{ChannelId, RoleId},
    prelude::TypeMapKey,
};
// use songbird::tracks::{LoopState, PlayMode, TrackState};
use strum::{Display, EnumIter, EnumString};
use tracing::{error, instrument};

use crate::{functions::is_default, here};

use self::functions::*;
pub use self::types::*;

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

    #[serde(default)]
    pub content_filtering: ContentFilteringConfig,

    #[serde(default)]
    pub embed_compressor: EmbedCompressorConfig,

    #[serde(skip)]
    pub talents: Vec<Talent>,
}

impl Config {
    #[instrument]
    pub async fn load(folder: &'static Path) -> anyhow::Result<Arc<Self>> {
        let config_path = folder.join("config.toml");
        let talents_path = folder.join("talents.toml");

        let mut config: Config = match load_toml_file_or_create_default(&config_path) {
            Ok(c) => c,
            Err(e) => {
                error!(?e, "Failed to open config file!");
                return Err(e);
            }
        };

        let talent_file: TalentFile = match load_toml_file_or_create_default(&talents_path) {
            Ok(t) => t,
            Err(e) => {
                error!(?e, "Failed to open talents file!");
                return Err(e);
            }
        };
        config.talents = talent_file.talents.into_iter().map(|t| t.into()).collect();

        Ok(Arc::new(config))
    }
}

impl TypeMapKey for Config {
    type Value = Self;
}

pub trait SaveToDatabase {
    const TABLE_NAME: &'static str;

    fn save_to_database(self, handle: &DatabaseHandle) -> anyhow::Result<()>;
}

pub trait LoadFromDatabase {
    const TABLE_NAME: &'static str;

    type Item;
    type ItemContainer: IntoIterator<Item = Self::Item>;

    fn load_from_database(handle: &DatabaseHandle) -> anyhow::Result<Self::ItemContainer>
    where
        Self::Item: Sized;
}

pub trait DatabaseOperations<'a, I: 'a>
where
    Self: Sized,
    Self: IntoIterator<Item = I>,
    I: Sized,
{
    type LoadItemContainer: IntoIterator<Item = I>;

    const TABLE_NAME: &'static str;
    const COLUMNS: &'static [(&'static str, &'static str, Option<&'static str>)];
    const TRUNCATE_TABLE: bool = false;

    fn into_row(item: I) -> Vec<Box<dyn ToSql>>;
    fn from_row(row: &rusqlite::Row) -> anyhow::Result<I>;

    fn create_table(handle: &DatabaseHandle) -> anyhow::Result<()> {
        handle
            .create_table(Self::TABLE_NAME, Self::COLUMNS)
            .context(here!())?;

        Ok(())
    }

    fn save_to_database(self, handle: &DatabaseHandle) -> anyhow::Result<()> {
        if Self::TRUNCATE_TABLE {
            handle.replace_table(
                Self::TABLE_NAME,
                Self::COLUMNS.iter().map(|(name, _, _)| *name),
                self.into_iter().map(Self::into_row),
            )
        } else {
            handle.insert_many(
                Self::TABLE_NAME,
                Self::COLUMNS.iter().map(|(name, _, _)| *name),
                self.into_iter().map(Self::into_row),
            )
        }
    }
    fn load_from_database(handle: &DatabaseHandle) -> anyhow::Result<Self::LoadItemContainer>
    where
        Self::LoadItemContainer: std::iter::FromIterator<I>,
    {
        match handle {
            DatabaseHandle::SQLite(h) => {
                let query_string = format!(
                    "SELECT {} FROM {}",
                    Self::COLUMNS
                        .iter()
                        .map(|(name, _, _)| *name)
                        .collect::<Vec<_>>()
                        .join(", "),
                    Self::TABLE_NAME
                );

                tracing::debug!("{}", query_string);

                let mut stmt = h.prepare(&query_string).context(here!())?;

                let results =
                    stmt.query_and_then([], |row| -> anyhow::Result<I> { Self::from_row(row) })?;

                results.collect()
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Birthday {
    pub day: u8,
    pub month: u8,
    pub year: Option<i16>,
}

impl Default for Birthday {
    fn default() -> Self {
        Self {
            day: 1,
            month: 1,
            year: None,
        }
    }
}

#[serde_as]
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(from = "TalentConfigData")]
pub struct Talent {
    pub name: String,
    pub emoji: String,
    pub icon: String,

    pub branch: HoloBranch,
    pub generation: HoloGeneration,

    pub birthday: Birthday,
    #[serde_as(as = "DisplayFromStr")]
    pub timezone: chrono_tz::Tz,

    pub youtube_ch_id: Option<holodex::model::id::ChannelId>,
    pub twitter_handle: Option<String>,
    pub twitter_id: Option<u64>,
    pub schedule_keyword: Option<String>,

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
            .with_ymd_and_hms(current_year, month as _, day as _, 0, 0, 0)
            .unwrap()
            .with_timezone(&Utc);

        if birthday < now {
            birthday.with_year(current_year + 1).unwrap_or(birthday)
        } else {
            birthday
        }
    }

    #[must_use]
    pub fn get_twitter_channel(&self, config: &Config) -> Option<ChannelId> {
        config
            .twitter
            .feeds
            .get(&self.branch)
            .and_then(|branch| branch.get(&self.generation))
            .copied()
    }
}

impl Display for Talent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl PartialEq for Talent {
    fn eq(&self, other: &Self) -> bool {
        self.twitter_id == other.twitter_id
    }
}

#[serde_as]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct TalentConfigData {
    pub name: String,
    pub emoji: String,
    pub icon: String,

    pub branch: HoloBranch,
    pub generation: HoloGeneration,

    #[serde(default)]
    pub birthday: Birthday,
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub timezone: Option<chrono_tz::Tz>,

    pub youtube_ch_id: Option<holodex::model::id::ChannelId>,
    pub twitter_handle: Option<String>,
    pub twitter_id: Option<u64>,
    pub schedule_keyword: Option<String>,

    #[serde(with = "SerHex::<CompactPfx>")]
    #[serde(default)]
    pub colour: u32,
    pub discord_role: Option<RoleId>,
}

impl From<TalentConfigData> for Talent {
    fn from(talent: TalentConfigData) -> Self {
        Self {
            name: talent.name,
            emoji: talent.emoji,
            icon: talent.icon,

            branch: talent.branch,
            generation: talent.generation,

            birthday: talent.birthday,
            timezone: talent.timezone.unwrap_or(Tz::UTC),

            youtube_ch_id: talent.youtube_ch_id,
            twitter_handle: talent.twitter_handle,
            twitter_id: talent.twitter_id,
            schedule_keyword: talent.schedule_keyword,

            colour: talent.colour,
            discord_role: talent.discord_role,
        }
    }
}

pub trait UserCollection {
    fn find_by_name(&self, name: &str) -> Option<&Talent>;
}

impl UserCollection for &[Talent] {
    fn find_by_name(&self, name: &str) -> Option<&Talent> {
        self.iter()
            .find(|u| u.name.to_lowercase().contains(&name.trim().to_lowercase()))
    }
}

impl UserCollection for Vec<Talent> {
    fn find_by_name(&self, name: &str) -> Option<&Talent> {
        self.iter()
            .find(|u| u.name.to_lowercase().contains(&name.trim().to_lowercase()))
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
    Default,
    EnumString,
    EnumIter,
    SerializeDisplay,
    DeserializeFromStr,
)]
#[non_exhaustive]
pub enum HoloBranch {
    #[default]
    HoloJP,
    HoloID,
    HoloEN,
    HolostarsJP,
    HolostarsEN,
    StaffJP,
    StaffID,
    StaffEN,
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
    Default,
    SerializeDisplay,
    DeserializeFromStr,
)]
#[non_exhaustive]
pub enum HoloGeneration {
    #[default]
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
    #[strum(to_string = "6th")]
    _6th,
    GAMERS,
    Myth,
    ProjectHope,
    Council,
    Tempus,
    Misc,
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

impl EmojiStats {
    pub fn total(&self) -> u64 {
        self.text_count + self.reaction_count
    }
}

impl std::ops::AddAssign for EmojiStats {
    fn add_assign(&mut self, rhs: Self) {
        self.text_count += rhs.text_count;
        self.reaction_count += rhs.reaction_count;
    }
}

impl std::ops::AddAssign<EmojiUsageSource> for EmojiStats {
    fn add_assign(&mut self, rhs: EmojiUsageSource) {
        match rhs {
            EmojiUsageSource::InText => self.text_count += 1,
            EmojiUsageSource::AsReaction => self.reaction_count += 1,
        }
    }
}

impl std::ops::AddAssign<EmojiUsageSource> for &mut EmojiStats {
    fn add_assign(&mut self, rhs: EmojiUsageSource) {
        match rhs {
            EmojiUsageSource::InText => self.text_count += 1,
            EmojiUsageSource::AsReaction => self.reaction_count += 1,
        }
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

impl std::ops::Add<EmojiUsageSource> for EmojiStats {
    type Output = Self;

    fn add(self, rhs: EmojiUsageSource) -> Self::Output {
        match rhs {
            EmojiUsageSource::InText => Self {
                text_count: self.text_count + 1,
                ..self
            },
            EmojiUsageSource::AsReaction => Self {
                reaction_count: self.reaction_count + 1,
                ..self
            },
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

#[derive(Debug)]
pub enum EntryEvent<K, V> {
    Added { key: K, value: V },
    Updated { key: K, value: V },
    Removed { key: K },
}

/* #[serde_as]
#[derive(Serialize, Deserialize)]
pub struct SavedMusicQueue {
    pub channel_id: ChannelId,
    #[serde_as(as = "Option<FromInto<TrackStateDef>>")]
    pub state: Option<TrackState>,
    pub tracks: Vec<EnqueuedItem>,
}

#[serde_as]
#[derive(Serialize, Deserialize)]
struct TrackStateDef {
    #[serde_as(as = "FromInto<PlayModeDef>")]
    pub playing: PlayMode,
    pub volume: f32,
    pub position: std::time::Duration,
    pub play_time: std::time::Duration,
    #[serde(with = "LoopStateDef")]
    pub loops: LoopState,
}

impl From<TrackState> for TrackStateDef {
    fn from(state: TrackState) -> Self {
        Self {
            playing: state.playing,
            volume: state.volume,
            position: state.position,
            play_time: state.play_time,
            loops: state.loops,
        }
    }
}

impl From<TrackStateDef> for TrackState {
    fn from(val: TrackStateDef) -> Self {
        TrackState {
            playing: val.playing,
            volume: val.volume,
            position: val.position,
            play_time: val.play_time,
            loops: val.loops,
        }
    }
}

#[derive(Serialize, Deserialize)]
enum PlayModeDef {
    Play,
    Pause,
    Stop,
    End,
}

impl From<PlayModeDef> for PlayMode {
    fn from(val: PlayModeDef) -> Self {
        match val {
            PlayModeDef::Play => PlayMode::Play,
            PlayModeDef::Pause => PlayMode::Pause,
            PlayModeDef::Stop => PlayMode::Stop,
            PlayModeDef::End => PlayMode::End,
        }
    }
}

impl From<PlayMode> for PlayModeDef {
    fn from(value: PlayMode) -> Self {
        match value {
            PlayMode::Play => PlayModeDef::Play,
            PlayMode::Pause => PlayModeDef::Pause,
            PlayMode::Stop => PlayModeDef::Stop,
            PlayMode::End => PlayModeDef::End,
            _ => panic!("Unsupported PlayMode"),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "LoopState")]
enum LoopStateDef {
    Infinite,
    Finite(usize),
}

impl FromSql for SavedMusicQueue {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        serde_json::from_slice(value.as_blob()?).map_err(|e| FromSqlError::Other(Box::new(e)))
    }
}

impl ToSql for SavedMusicQueue {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Owned(Value::Blob(
            serde_json::to_vec(self)
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
        )))
    }
}

impl DatabaseOperations<'_, (GuildId, SavedMusicQueue)> for HashMap<GuildId, SavedMusicQueue> {
    type LoadItemContainer = HashMap<GuildId, SavedMusicQueue>;

    const TRUNCATE_TABLE: bool = true;
    const TABLE_NAME: &'static str = "MusicQueues";
    const COLUMNS: &'static [(&'static str, &'static str, Option<&'static str>)] = &[
        ("guild_id", "INTEGER", Some("PRIMARY KEY")),
        ("queue", "BLOB", Some("NOT NULL")),
    ];

    fn into_row((guild_id, queue): (GuildId, SavedMusicQueue)) -> Vec<Box<dyn ToSql>> {
        vec![Box::new(guild_id.0), Box::new(queue)]
    }

    fn from_row(row: &rusqlite::Row) -> anyhow::Result<(GuildId, SavedMusicQueue)> {
        Ok((
            row.get::<_, u64>("guild_id")
                .map(GuildId)
                .context(here!())?,
            row.get("queue").context(here!())?,
        ))
    }
} */
