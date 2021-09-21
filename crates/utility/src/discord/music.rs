use std::{
    collections::{HashMap, VecDeque},
    time::Duration,
};

use chrono::{DateTime, Utc};
use serenity::{
    model::{
        guild::Member,
        id::{GuildId, UserId},
    },
    utils::Colour,
};
use songbird::tracks::{TrackHandle, TrackQueue};

type Ctx = serenity::client::Context;

#[derive(Debug, Clone, Default)]
pub struct MusicData {
    pub queues: HashMap<GuildId, TrackQueue>,
    pub forced_songs: HashMap<GuildId, Option<TrackHandle>>,
    pub misc_data: HashMap<GuildId, MiscData>,
}

impl MusicData {
    pub fn get_current(&self, guild_id: &GuildId) -> CurrentTrack {
        if !self.is_guild_registered(guild_id) {
            return CurrentTrack::None;
        }

        if let Some(forced_track) = self.forced_songs.get(guild_id).unwrap() {
            return CurrentTrack::Forced(forced_track.clone());
        }

        if let Some(queued_track) = self.queues.get(guild_id).unwrap().current() {
            return CurrentTrack::InQueue(queued_track);
        }

        CurrentTrack::None
    }

    pub fn get_forced(&self, guild_id: &GuildId) -> Option<&TrackHandle> {
        if !self.is_guild_registered(guild_id) {
            return None;
        }

        self.forced_songs.get(guild_id).unwrap().as_ref()
    }

    pub fn get_source(&self, guild_id: &GuildId) -> CurrentSource<'_> {
        if !self.is_guild_registered(guild_id) {
            return CurrentSource::None;
        }

        if let Some(forced_track) = self.forced_songs.get(guild_id).unwrap() {
            return CurrentSource::Forced(forced_track);
        }

        let queue = self.queues.get(guild_id).unwrap();

        if !queue.is_empty() {
            return CurrentSource::Queue(queue);
        }

        CurrentSource::None
    }

    pub fn is_guild_registered(&self, guild_id: &GuildId) -> bool {
        self.queues.contains_key(guild_id)
    }

    pub fn register_guild(&mut self, guild_id: GuildId) {
        if self.queues.contains_key(&guild_id) {
            return;
        }

        self.queues.insert(guild_id, TrackQueue::new());
        self.forced_songs.insert(guild_id, None);
        self.misc_data.insert(guild_id, MiscData::default());
    }

    pub fn deregister_guild(&mut self, guild_id: &GuildId) -> anyhow::Result<()> {
        self.stop(guild_id)?;

        self.queues.remove(guild_id);
        self.forced_songs.remove(guild_id);
        self.misc_data.remove(guild_id);

        Ok(())
    }

    pub fn stop(&self, guild_id: &GuildId) -> anyhow::Result<()> {
        if let Some(queue) = self.queues.get(guild_id) {
            queue.stop();
        }

        if let Some(Some(forced_song)) = self.forced_songs.get(guild_id) {
            forced_song.stop()?;
        }

        Ok(())
    }

    pub fn set_volume(&mut self, guild_id: &GuildId, volume: f32) -> anyhow::Result<()> {
        if let Some(misc_data) = self.misc_data.get_mut(guild_id) {
            misc_data.volume = volume;
        }

        if let Some(Some(forced_song)) = self.forced_songs.get(guild_id) {
            forced_song.set_volume(volume)?;
        }

        if let Some(queue) = self.queues.get(guild_id) {
            queue.modify_queue(|q| q.iter_mut().try_for_each(|t| t.set_volume(volume)))?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct MiscData {
    pub queue_is_looping: bool,
    pub volume: f32,
}

impl Default for MiscData {
    fn default() -> Self {
        Self {
            queue_is_looping: false,
            volume: 1.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TrackMetaData {
    pub added_by: UserId,
    pub added_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct TrackMetaDataFull {
    pub added_by: Member,
    pub added_at: DateTime<Utc>,
    pub member_colour: Option<Colour>,
}

impl TrackMetaData {
    pub async fn fetch_data(
        &self,
        ctx: &Ctx,
        guild_id: &GuildId,
    ) -> anyhow::Result<TrackMetaDataFull> {
        let member = guild_id.member(&ctx.http, self.added_by).await?;

        Ok(TrackMetaDataFull {
            member_colour: member.colour(&ctx.cache).await,
            added_by: member,
            added_at: self.added_at,
        })
    }
}

#[derive(Debug)]
pub enum CurrentTrack {
    None,
    InQueue(TrackHandle),
    Forced(TrackHandle),
}

#[derive(Debug)]
pub enum CurrentSource<'a> {
    None,
    Queue(&'a TrackQueue),
    Forced(&'a TrackHandle),
}

#[derive(Debug)]
pub enum QueueRemovalCondition {
    All,
    Duplicates,
    Indices(String),
    FromUser(UserId),
}
