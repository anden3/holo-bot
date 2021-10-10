use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use anyhow::Context;
use chrono::Duration;
use itertools::Itertools;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr, DurationSeconds};
use serenity::{
    builder::CreateEmbed,
    model::id::{ChannelId, EmojiId, GuildId, RoleId, UserId},
    utils::Color,
};

use crate::{functions::default_true, here, types::TranslatorType};

use super::{
    functions::{get_map_updates, get_nested_map_updates, get_set_updates},
    ConfigDiff, ConfigUpdate, HoloBranch, HoloGeneration, Talent,
};

#[derive(Debug, Deserialize, Serialize, Default)]
pub(crate) struct TalentFile {
    pub talents: Vec<Talent>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct BlockedEntities {
    #[serde(default)]
    pub users: HashSet<UserId>,
    #[serde(default)]
    pub servers: HashSet<GuildId>,
    #[serde(default)]
    pub channels: HashSet<ChannelId>,
}

impl ConfigDiff for BlockedEntities {
    fn diff(&self, new: &Self) -> Vec<ConfigUpdate> {
        let mut changes = Vec::new();

        changes.extend(get_set_updates(
            &self.users,
            &new.users,
            ConfigUpdate::UserBlocked,
            ConfigUpdate::UserUnblocked,
        ));
        changes.extend(get_set_updates(
            &self.channels,
            &new.channels,
            ConfigUpdate::ChannelBlocked,
            ConfigUpdate::ChannelUnblocked,
        ));
        changes.extend(get_set_updates(
            &self.servers,
            &new.servers,
            ConfigUpdate::GuildBlocked,
            ConfigUpdate::GuildUnblocked,
        ));

        changes
    }
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

impl ConfigDiff for Database {
    fn diff(&self, new: &Self) -> Vec<ConfigUpdate> {
        let mut changes = Vec::new();

        match (self, new) {
            (Database::SQLite { path: old_path }, Database::SQLite { path: new_path }) => {
                if old_path != new_path {
                    changes.push(ConfigUpdate::DatabaseSQLiteRenamed {
                        from: old_path.clone(),
                        to: new_path.clone(),
                    });
                }
            }
        }

        changes
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

impl ConfigDiff for StreamTrackingConfig {
    fn diff(&self, new: &Self) -> Vec<ConfigUpdate> {
        let mut changes = Vec::new();

        if self.enabled != new.enabled {
            if new.enabled {
                changes.push(ConfigUpdate::StreamTrackingEnabled);
            } else {
                changes.push(ConfigUpdate::StreamTrackingDisabled);
            }
        }

        if self.holodex_token != new.holodex_token {
            changes.push(ConfigUpdate::HolodexTokenChanged(new.holodex_token.clone()));
        }

        changes.extend(self.alerts.diff(&new.alerts));
        changes.extend(self.chat.diff(&new.chat));

        changes
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct StreamAlertsConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub channel: ChannelId,
}

impl ConfigDiff for StreamAlertsConfig {
    fn diff(&self, new: &Self) -> Vec<ConfigUpdate> {
        let mut changes = Vec::new();

        if self.enabled != new.enabled {
            if new.enabled {
                changes.push(ConfigUpdate::StreamAlertsEnabled);
            } else {
                changes.push(ConfigUpdate::StreamAlertsDisabled);
            }
        }

        if self.channel != new.channel {
            changes.push(ConfigUpdate::StreamAlertsChannelChanged(new.channel));
        }

        changes
    }
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

impl ConfigDiff for StreamChatConfig {
    fn diff(&self, new: &Self) -> Vec<ConfigUpdate> {
        let mut changes = Vec::new();

        if self.enabled != new.enabled {
            if new.enabled {
                changes.push(ConfigUpdate::StreamChatsEnabled);
            } else {
                changes.push(ConfigUpdate::StreamChatsDisabled);
            }
        }

        if self.category != new.category {
            changes.push(ConfigUpdate::StreamChatCategoryChanged(new.category));
        }

        match (self.logging_channel, new.logging_channel) {
            (None, None) => (),
            (None, Some(ch)) => changes.push(ConfigUpdate::StreamChatLoggingEnabled(ch)),
            (Some(_), None) => changes.push(ConfigUpdate::StreamChatLoggingDisabled),
            (Some(old), Some(new)) => {
                if old != new {
                    changes.push(ConfigUpdate::StreamChatLoggingChannelChanged(new));
                }
            }
        }

        changes.extend(get_map_updates(
            &self.post_stream_discussion,
            &new.post_stream_discussion,
            |b| ConfigUpdate::StreamChatPostStreamDiscussionChanged {
                branch: b,
                channel: None,
            },
            |b, c| ConfigUpdate::StreamChatPostStreamDiscussionChanged {
                branch: b,
                channel: Some(c),
            },
            |b, c| ConfigUpdate::StreamChatPostStreamDiscussionChanged {
                branch: b,
                channel: Some(c),
            },
        ));

        changes
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct MusicBotConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub channel: ChannelId,
}

impl ConfigDiff for MusicBotConfig {
    fn diff(&self, new: &Self) -> Vec<ConfigUpdate> {
        let mut changes = Vec::new();

        if self.enabled != new.enabled {
            if new.enabled {
                changes.push(ConfigUpdate::MusicBotEnabled);
            } else {
                changes.push(ConfigUpdate::MusicBotDisabled);
            }
        }

        if self.channel != new.channel {
            changes.push(ConfigUpdate::MusicBotChannelChanged(new.channel));
        }

        changes
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct BirthdayAlertsConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub channel: ChannelId,
}

impl ConfigDiff for BirthdayAlertsConfig {
    fn diff(&self, new: &Self) -> Vec<ConfigUpdate> {
        let mut changes = Vec::new();

        if self.enabled != new.enabled {
            if new.enabled {
                changes.push(ConfigUpdate::BirthdayAlertsEnabled);
            } else {
                changes.push(ConfigUpdate::BirthdayAlertsDisabled);
            }
        }

        if self.channel != new.channel {
            changes.push(ConfigUpdate::BirthdayAlertsChannelChanged(new.channel));
        }

        changes
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct EmojiTrackingConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl ConfigDiff for EmojiTrackingConfig {
    fn diff(&self, new: &Self) -> Vec<ConfigUpdate> {
        let mut changes = Vec::new();

        if self.enabled != new.enabled {
            if new.enabled {
                changes.push(ConfigUpdate::EmojiTrackingEnabled);
            } else {
                changes.push(ConfigUpdate::EmojiTrackingDisabled);
            }
        }

        changes
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct MemeCreationConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub imgflip_user: String,
    pub imgflip_pass: String,
}

impl ConfigDiff for MemeCreationConfig {
    fn diff(&self, new: &Self) -> Vec<ConfigUpdate> {
        let mut changes = Vec::new();

        if self.enabled != new.enabled {
            if new.enabled {
                changes.push(ConfigUpdate::MemeCreationEnabled);
            } else {
                changes.push(ConfigUpdate::MemeCreationDisabled);
            }
        }

        if self.imgflip_user != new.imgflip_user || self.imgflip_pass != new.imgflip_pass {
            changes.push(ConfigUpdate::ImgflipCredentialsChanged {
                user: new.imgflip_user.clone(),
                pass: new.imgflip_pass.clone(),
            });
        }

        changes
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct AiChatbotConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub openai_token: String,
}

impl ConfigDiff for AiChatbotConfig {
    fn diff(&self, new: &Self) -> Vec<ConfigUpdate> {
        let mut changes = Vec::new();

        if self.enabled != new.enabled {
            if new.enabled {
                changes.push(ConfigUpdate::AiChatbotEnabled);
            } else {
                changes.push(ConfigUpdate::AiChatbotDisabled);
            }
        }

        if self.openai_token != new.openai_token {
            changes.push(ConfigUpdate::OpenAiTokenChanged(new.openai_token.clone()));
        }

        changes
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ReminderConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl ConfigDiff for ReminderConfig {
    fn diff(&self, new: &Self) -> Vec<ConfigUpdate> {
        let mut changes = Vec::new();

        if self.enabled != new.enabled {
            if new.enabled {
                changes.push(ConfigUpdate::RemindersEnabled);
            } else {
                changes.push(ConfigUpdate::RemindersDisabled);
            }
        }

        changes
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct QuoteConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl ConfigDiff for QuoteConfig {
    fn diff(&self, new: &Self) -> Vec<ConfigUpdate> {
        let mut changes = Vec::new();

        if self.enabled != new.enabled {
            if new.enabled {
                changes.push(ConfigUpdate::QuotesEnabled);
            } else {
                changes.push(ConfigUpdate::QuotesDisabled);
            }
        }

        changes
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
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

impl ConfigDiff for TwitterConfig {
    fn diff(&self, new: &Self) -> Vec<ConfigUpdate> {
        let mut changes = Vec::new();

        if self.enabled != new.enabled {
            if new.enabled {
                changes.push(ConfigUpdate::TwitterEnabled(new.clone()));
            } else {
                changes.push(ConfigUpdate::TwitterDisabled);
            }
        }

        if self.token != new.token {
            changes.push(ConfigUpdate::TwitterTokenChanged(new.token.clone()));
        }

        changes.extend(self.schedule_updates.diff(&new.schedule_updates));

        changes.extend(get_nested_map_updates(
            &self.feeds,
            &new.feeds,
            |(b, g)| ConfigUpdate::TwitterFeedRemoved {
                branch: b,
                generation: g,
            },
            |(b, g), c| ConfigUpdate::TwitterFeedAdded {
                branch: b,
                generation: g,
                channel: c,
            },
            |(b, g), c| ConfigUpdate::TwitterFeedChanged {
                branch: b,
                generation: g,
                new_channel: c,
            },
        ));

        changes.extend(get_map_updates(
            &self.feed_translation,
            &new.feed_translation,
            ConfigUpdate::TranslatorRemoved,
            ConfigUpdate::TranslatorAdded,
            ConfigUpdate::TranslatorChanged,
        ));

        changes
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
pub struct ScheduleUpdateConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub channel: ChannelId,
}

impl ConfigDiff for ScheduleUpdateConfig {
    fn diff(&self, new: &Self) -> Vec<ConfigUpdate> {
        let mut changes = Vec::new();

        if self.enabled != new.enabled {
            if new.enabled {
                changes.push(ConfigUpdate::ScheduleUpdatesEnabled);
            } else {
                changes.push(ConfigUpdate::ScheduleUpdatesDisabled);
            }
        }

        if self.channel != new.channel {
            changes.push(ConfigUpdate::ScheduleUpdatesChannelChanged(new.channel));
        }

        changes
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
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

impl ConfigDiff for ReactTempMuteConfig {
    fn diff(&self, new: &Self) -> Vec<ConfigUpdate> {
        let mut changes = Vec::new();

        if self.enabled != new.enabled {
            if new.enabled {
                changes.push(ConfigUpdate::ReactTempMuteEnabled);
            } else {
                changes.push(ConfigUpdate::ReactTempMuteDisabled);
            }
        }

        if self.mute_role != new.mute_role {
            changes.push(ConfigUpdate::ReactTempMuteRoleChanged(new.mute_role));
        }

        if self.required_reaction_count != new.required_reaction_count {
            changes.push(ConfigUpdate::ReactTempMuteReactionCountChanged(
                new.required_reaction_count,
            ));
        }

        if self.excessive_mute_threshold != new.excessive_mute_threshold {
            changes.push(ConfigUpdate::ReactTempMuteExcessiveMuteThresholdChanged(
                new.excessive_mute_threshold,
            ));
        }

        if self.mute_duration != new.mute_duration {
            changes.push(ConfigUpdate::ReactTempMuteDurationChanged(
                new.mute_duration,
            ));
        }

        if self.eligibility_duration != new.eligibility_duration {
            changes.push(ConfigUpdate::ReactTempMuteEligibilityChanged(
                new.eligibility_duration,
            ));
        }

        if self.reactions != new.reactions {
            changes.push(ConfigUpdate::ReactTempMuteReactionsChanged(
                new.reactions.clone(),
            ));
        }

        match (self.logging_channel, new.logging_channel) {
            (None, None) => (),
            (None, Some(ch)) => changes.push(ConfigUpdate::ReactTempMuteLoggingEnabled(ch)),
            (Some(_), None) => changes.push(ConfigUpdate::ReactTempMuteLoggingDisabled),
            (Some(old), Some(new)) => {
                if old != new {
                    changes.push(ConfigUpdate::ReactTempMuteLoggingChannelChanged(new));
                }
            }
        }

        changes
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ContentFilteringConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub public_log_image: Option<String>,
    pub blacklisted_yt_channels: HashMap<String, BlacklistedYTChannel>,
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
            .colour(Color::RED)
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
