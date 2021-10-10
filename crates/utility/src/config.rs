mod functions;
mod types;

use std::{
    collections::HashSet,
    fmt::Display,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

use anyhow::{anyhow, Context};
use chrono::{prelude::*, Duration};
use chrono_tz::Tz;
use notify::{event::ModifyKind, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use regex::Regex;
use rusqlite::{
    types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, Value, ValueRef},
    ToSql,
};
use serde::{Deserialize, Serialize};
use serde_hex::{CompactPfx, SerHex};
use serde_with::{serde_as, DeserializeFromStr, DisplayFromStr, SerializeDisplay};
use serenity::{
    builder::CreateEmbed,
    model::id::{ChannelId, EmojiId, GuildId, RoleId, UserId},
    prelude::TypeMapKey,
};
use strum_macros::{Display, EnumIter, EnumString, ToString};
use tokio::sync::broadcast;
use tracing::{debug, error, instrument, warn};

use crate::{functions::is_default, here, regex, types::TranslatorType};

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

    #[serde(skip)]
    pub talents: Vec<Talent>,

    #[serde(skip)]
    pub updates: Option<broadcast::Sender<ConfigUpdate>>,
}

impl Config {
    #[instrument]
    pub async fn load(folder: &'static Path) -> anyhow::Result<(Arc<Self>, RecommendedWatcher)> {
        let config_path = folder.join("config.toml");
        let talents_path = folder.join("talents.toml");

        let mut config: Config = match load_toml_file_or_create_default(&config_path) {
            Ok(c) => c,
            Err(e) => {
                error!(?e, "Failed to open config file!");
                return Err(e);
            }
        };

        let handle = config.database.get_handle()?;
        Database::initialize_tables(&handle)?;

        let talents: TalentFile = match load_toml_file_or_create_default(&talents_path) {
            Ok(t) => t,
            Err(e) => {
                error!(?e, "Failed to open talents file!");
                return Err(e);
            }
        };
        config.talents = talents.talents;

        let (cfg_updates, _cfg_update_recv) = broadcast::channel(64);

        let config_watcher =
            match Self::config_notifier(folder, config.clone(), cfg_updates.clone()).await {
                Ok(n) => n,
                Err(e) => {
                    error!(?e, "Failed to create config notifier!");
                    return Err(e);
                }
            };

        config.updates = Some(cfg_updates);

        Ok((Arc::new(config), config_watcher))
    }

    async fn config_notifier(
        folder: &Path,
        mut config: Config,
        sender: broadcast::Sender<ConfigUpdate>,
    ) -> anyhow::Result<RecommendedWatcher> {
        enum FileChanged<'a> {
            Config(&'a Path),
            Talents(&'a Path),
        }

        impl<'a> FileChanged<'a> {
            fn from_path(path: &'a Path) -> Option<Self> {
                if path.ends_with("config.toml") {
                    Some(FileChanged::Config(path))
                } else if path.ends_with("talents.toml") {
                    Some(FileChanged::Talents(path))
                } else {
                    None
                }
            }

            fn get_path(&self) -> &Path {
                match self {
                    FileChanged::Config(p) => p,
                    FileChanged::Talents(p) => p,
                }
            }
        }

        let mut watcher =
            notify::recommended_watcher(move |e: Result<notify::Event, notify::Error>| {
                debug!(?e);

                let event = match e {
                    Ok(e) => e,
                    Err(e) => {
                        error!(?e, "Watch error!");
                        return;
                    }
                };

                debug!("Event: {:#?}", event);

                let files = event
                    .paths
                    .iter()
                    .filter_map(|p| FileChanged::from_path(p.as_path()));

                for file in files {
                    let path = file.get_path();

                    match &event.kind {
                        EventKind::Modify(ModifyKind::Data(_)) => {
                            let new_config = match load_toml_file_or_create_default(path) {
                                Ok(c) => c,
                                Err(e) => {
                                    error!(?e, "Failed to open config file!");
                                    return;
                                }
                            };

                            let changes = config.diff(&new_config);

                            debug!("{:#?}", changes);

                            if sender.receiver_count() > 0 {
                                for change in changes {
                                    if let Err(e) = sender.send(change) {
                                        error!(?e, "Failed to send config update!");
                                    }
                                }
                            }

                            config = new_config;
                        }
                        EventKind::Remove(_) => todo!(),
                        e => {
                            warn!(?e, "Unhandled event kind!");
                        }
                    }
                }
            })?;

        watcher.watch(&folder.join("config.toml"), RecursiveMode::NonRecursive)?;
        watcher.watch(&folder.join("talents.toml"), RecursiveMode::NonRecursive)?;

        Ok(watcher)
    }
}

impl TypeMapKey for Config {
    type Value = Self;
}

#[derive(Debug, Clone)]
pub enum ConfigUpdate {
    DiscordTokenChanged(String),

    UserBlocked(UserId),
    UserUnblocked(UserId),

    ChannelBlocked(ChannelId),
    ChannelUnblocked(ChannelId),

    GuildBlocked(GuildId),
    GuildUnblocked(GuildId),

    DatabaseSQLiteRenamed {
        from: PathBuf,
        to: PathBuf,
    },

    StreamTrackingEnabled,
    StreamTrackingDisabled,
    StreamAlertsEnabled,
    StreamAlertsDisabled,
    StreamChatsEnabled,
    StreamChatsDisabled,
    StreamChatLoggingEnabled(ChannelId),
    StreamChatLoggingDisabled,

    HolodexTokenChanged(String),
    StreamAlertsChannelChanged(ChannelId),
    StreamChatCategoryChanged(ChannelId),
    StreamChatLoggingChannelChanged(ChannelId),
    StreamChatPostStreamDiscussionChanged {
        branch: HoloBranch,
        channel: Option<ChannelId>,
    },

    MusicBotEnabled,
    MusicBotDisabled,
    MusicBotChannelChanged(ChannelId),

    BirthdayAlertsEnabled,
    BirthdayAlertsDisabled,
    BirthdayAlertsChannelChanged(ChannelId),

    EmojiTrackingEnabled,
    EmojiTrackingDisabled,

    MemeCreationEnabled,
    MemeCreationDisabled,
    ImgflipCredentialsChanged {
        user: String,
        pass: String,
    },

    AiChatbotEnabled,
    AiChatbotDisabled,
    OpenAiTokenChanged(String),

    RemindersEnabled,
    RemindersDisabled,

    QuotesEnabled,
    QuotesDisabled,

    TwitterEnabled(TwitterConfig),
    TwitterDisabled,
    TwitterTokenChanged(String),

    TwitterFeedAdded {
        branch: HoloBranch,
        generation: HoloGeneration,
        channel: ChannelId,
    },
    TwitterFeedRemoved {
        branch: HoloBranch,
        generation: HoloGeneration,
    },
    TwitterFeedChanged {
        branch: HoloBranch,
        generation: HoloGeneration,
        new_channel: ChannelId,
    },

    ScheduleUpdatesEnabled,
    ScheduleUpdatesDisabled,
    ScheduleUpdatesChannelChanged(ChannelId),

    TranslatorAdded(TranslatorType, TranslatorConfig),
    TranslatorRemoved(TranslatorType),
    TranslatorChanged(TranslatorType, TranslatorConfig),

    ReactTempMuteEnabled,
    ReactTempMuteDisabled,
    ReactTempMuteRoleChanged(RoleId),
    ReactTempMuteReactionCountChanged(usize),
    ReactTempMuteReactionsChanged(HashSet<EmojiId>),
    ReactTempMuteExcessiveMuteThresholdChanged(usize),
    ReactTempMuteDurationChanged(Duration),
    ReactTempMuteEligibilityChanged(Duration),

    ReactTempMuteLoggingEnabled(ChannelId),
    ReactTempMuteLoggingDisabled,
    ReactTempMuteLoggingChannelChanged(ChannelId),
}

trait ConfigDiff {
    fn diff(&self, new: &Self) -> Vec<ConfigUpdate>;
}

impl ConfigDiff for Config {
    fn diff(&self, new: &Self) -> Vec<ConfigUpdate> {
        let mut changes = Vec::new();

        if self.discord_token != new.discord_token {
            changes.push(ConfigUpdate::DiscordTokenChanged(new.discord_token.clone()));
        }

        changes.extend(self.blocked.diff(&new.blocked));
        changes.extend(self.database.diff(&new.database));
        changes.extend(self.stream_tracking.diff(&new.stream_tracking));
        changes.extend(self.music_bot.diff(&new.music_bot));
        changes.extend(self.meme_creation.diff(&new.meme_creation));
        changes.extend(self.emoji_tracking.diff(&new.emoji_tracking));
        changes.extend(self.ai_chatbot.diff(&new.ai_chatbot));
        changes.extend(self.reminders.diff(&new.reminders));
        changes.extend(self.quotes.diff(&new.quotes));
        changes.extend(self.twitter.diff(&new.twitter));
        changes.extend(self.react_temp_mute.diff(&new.react_temp_mute));

        changes
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
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
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

impl Display for Talent {
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

impl Default for HoloBranch {
    fn default() -> Self {
        HoloBranch::HoloJP
    }
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

impl Default for HoloGeneration {
    fn default() -> Self {
        HoloGeneration::_0th
    }
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
