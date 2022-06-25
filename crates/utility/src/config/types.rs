use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use anyhow::Context;
use chrono::Duration;
use itertools::Itertools;
use rusqlite::{params_from_iter, Connection, OptionalExtension, ToSql};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr, DurationSeconds};
use serenity::{
    builder::CreateEmbed,
    model::{
        channel::Message,
        id::{ChannelId, EmojiId, GuildId, RoleId, UserId},
        mention::Mention,
    },
    utils::Colour,
};

use crate::{functions::default_true, here, types::TranslatorType};

use super::{HoloBranch, HoloGeneration, TalentConfigData};

#[derive(Debug, Deserialize, Serialize, Default)]
pub(crate) struct TalentFile {
    pub talents: Vec<TalentConfigData>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
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
    SQLite { path: PathBuf },
}

impl Default for Database {
    fn default() -> Self {
        Self::SQLite {
            path: Path::new("").to_owned(),
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
}

#[derive(Debug)]
pub enum DatabaseHandle {
    SQLite(Connection),
}

impl DatabaseHandle {
    pub fn create_table(
        &self,
        name: &str,
        schema: &[(&str, &str, Option<&str>)],
    ) -> anyhow::Result<bool> {
        match self {
            DatabaseHandle::SQLite(h) => h
                .execute(
                    &format!(
                        "CREATE TABLE IF NOT EXISTS {} ({})",
                        name,
                        &schema
                            .iter()
                            .map(|(k, v, m)| format!("{} {} {}", k, v, m.unwrap_or_default()))
                            .join(", ")
                    ),
                    [],
                )
                .map(|n| n > 0)
                .context(here!()),
        }
    }

    pub fn replace_table<'a, K, V>(&self, table: &str, keys: K, values: V) -> anyhow::Result<()>
    where
        K: Iterator<Item = &'a str> + Clone,
        V: Iterator<Item = Vec<Box<dyn ToSql>>>,
    {
        self.truncate_table(table)?;
        self.insert_many(table, keys, values)?;

        Ok(())
    }

    pub fn rename_table(&self, table: &str, new_name: &str) -> anyhow::Result<bool> {
        if !self.contains_table(table).context(here!())? {
            return Ok(false);
        }

        match self {
            DatabaseHandle::SQLite(h) => Ok(h
                .execute(&format!("ALTER TABLE {} RENAME TO {}", table, new_name), [])
                .context(here!())?
                == 1),
        }
    }

    pub fn contains_table(&self, table: &str) -> anyhow::Result<bool> {
        match self {
            DatabaseHandle::SQLite(h) => Ok({
                /* h.execute(
                    "SELECT name FROM sqlite_master WHERE type='table' AND name=?",
                    [table],
                )? */

                h.query_row_and_then(
                    "SELECT name FROM sqlite_master WHERE type='table' AND name=?;",
                    &[table],
                    |row: &rusqlite::Row| -> rusqlite::Result<bool> {
                        Ok(row.get::<_, String>(0)? == *table)
                    },
                )
                .optional()?
                .unwrap_or_default()
            }),
        }
    }

    pub fn truncate_table(&self, table: &str) -> anyhow::Result<bool> {
        match self {
            DatabaseHandle::SQLite(h) => h
                .execute(&format!("DELETE FROM {}", table), [])
                .map(|n| n > 0)
                .context(here!()),
        }
    }

    pub fn insert<'a, K, V>(&self, table: &str, keys: K, values: V) -> anyhow::Result<()>
    where
        K: Iterator<Item = &'a str> + Clone,
        V: Iterator<Item = &'a dyn ToSql>,
    {
        match self {
            DatabaseHandle::SQLite(h) => {
                let query_string = format!(
                    "INSERT OR REPLACE INTO {} ({}) VALUES ({})",
                    table,
                    keys.clone().join(", "),
                    keys.map(|_| "?").join(", "),
                );

                let mut stmt = h.prepare_cached(&query_string)?;
                let tx = h.unchecked_transaction()?;

                stmt.execute(params_from_iter(values))?;

                tx.commit()?;
            }
        }

        Ok(())
    }

    pub fn insert_many<'a, K, V>(&self, table: &str, keys: K, values: V) -> anyhow::Result<()>
    where
        K: Iterator<Item = &'a str> + Clone,
        V: Iterator<Item = Vec<Box<dyn ToSql>>>,
    {
        match self {
            DatabaseHandle::SQLite(h) => {
                let query_string = format!(
                    "INSERT OR REPLACE INTO {} ({}) VALUES ({})",
                    table,
                    keys.clone().join(", "),
                    keys.map(|_| "?").join(", "),
                );

                let mut stmt = h.prepare_cached(&query_string)?;
                let tx = h.unchecked_transaction()?;

                for values in values {
                    stmt.execute(params_from_iter(values))?;
                }

                tx.commit()?;
            }
        }

        Ok(())
    }
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

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
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

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
pub struct ScheduleUpdateConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub channel: ChannelId,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ContentFilteringConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub mute_role: RoleId,
    pub public_log_image: Option<String>,
    pub logging_channel: ChannelId,
    pub staff_role: Option<RoleId>,

    #[serde(default)]
    pub doxxing_channels: HashSet<Cow<'static, str>>,
    #[serde(default)]
    pub blacklisted_yt_channels: HashMap<Cow<'static, str>, BlacklistedYTChannel>,
}

pub enum ContentFilterAction {
    DeleteMsg,
    Log(CreateEmbed),
    LogStaff(CreateEmbed),
    LogStaffNotify(CreateEmbed),
    Mute(Duration),
    Ban(String),
}

pub enum ContentFilterResult<'a> {
    NotFiltered,
    ContainsDoxxingChannel(Vec<ContentFilterAction>),
    ContainsBlacklistedYTChannel(Vec<ContentFilterAction>, Vec<&'a BlacklistedYTChannel>),
    ContainsBlacklistedWord(Vec<ContentFilterAction>, &'a [&'a str]),
}

impl ContentFilterResult<'_> {
    pub fn into_actions(self) -> Vec<ContentFilterAction> {
        match self {
            ContentFilterResult::NotFiltered => Vec::new(),
            ContentFilterResult::ContainsDoxxingChannel(actions) => actions,
            ContentFilterResult::ContainsBlacklistedYTChannel(actions, _) => actions,
            ContentFilterResult::ContainsBlacklistedWord(actions, _) => actions,
        }
    }
}

impl ContentFilteringConfig {
    pub fn filter<'a>(&'a self, msg: &'a Message) -> ContentFilterResult<'a> {
        use ContentFilterAction::*;

        if !self.enabled || msg.author.bot {
            return ContentFilterResult::NotFiltered;
        }

        let yt_channels_in_msg = msg
            .embeds
            .iter()
            .filter_map(|e| {
                e.author
                    .as_ref()
                    .and_then(|a| a.url.as_ref())
                    .and_then(|u| u.strip_prefix("https://www.youtube.com/channel/"))
                    .map(Cow::Borrowed)
            })
            .collect::<HashSet<_>>();

        let doxxers_in_msg = yt_channels_in_msg
            .intersection(&self.doxxing_channels)
            .collect::<Vec<_>>();

        if !doxxers_in_msg.is_empty() {
            let mut actions = vec![DeleteMsg];

            actions.extend(doxxers_in_msg.into_iter().map(|d| {
                LogStaffNotify({
                    let mut embed = CreateEmbed::default();

                    embed.author(|a| a.name("Content Filtering"));
                    embed.title("Video from known doxxer removed");
                    embed.colour(Colour::RED);

                    embed.fields([
                        (
                            "Posted by",
                            Mention::from(msg.author.id).to_string().as_str(),
                            true,
                        ),
                        ("Channel", d.as_ref(), true),
                        ("Message", &msg.content, true),
                    ]);

                    embed
                })
            }));

            return ContentFilterResult::ContainsDoxxingChannel(actions);
        }

        let blacklisted_channels_in_msg = yt_channels_in_msg
            .iter()
            .filter_map(|c| self.blacklisted_yt_channels.get(c))
            .collect::<Vec<_>>();

        if !blacklisted_channels_in_msg.is_empty() {
            let mut actions = vec![DeleteMsg];

            actions.extend(
                blacklisted_channels_in_msg
                    .iter()
                    .map(|c| Log(c.to_embed(self))),
            );

            return ContentFilterResult::ContainsBlacklistedYTChannel(
                actions,
                blacklisted_channels_in_msg,
            );
        }

        ContentFilterResult::NotFiltered
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct BlacklistedYTChannel {
    pub name: String,
    pub reason: String,
    pub sources: Vec<String>,
}

impl BlacklistedYTChannel {
    pub fn to_embed(&self, config: &ContentFilteringConfig) -> CreateEmbed {
        let mut embed = CreateEmbed::default();
        embed
            .title("Video from blacklisted YT channel removed")
            .author(|a| a.name("Content Filtering"))
            .colour(Colour::RED)
            .fields([
                ("Name", &self.name, true),
                ("Reason for blacklist", &self.reason, true),
                (
                    "Sources",
                    &self
                        .sources
                        .iter()
                        .enumerate()
                        .map(|(i, s)| format!("[[{}]]({})", i + 1, s))
                        .join(" - "),
                    true,
                ),
            ]);

        if let Some(thumbnail) = &config.public_log_image {
            embed.thumbnail(thumbnail);
        }

        embed
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct EmbedCompressorConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}
