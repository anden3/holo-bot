use songbird::input::Metadata;

use super::{
    metadata::{TrackMetaData, TrackMetaDataFull},
    prelude::*,
};

#[derive(Debug, Clone)]
pub struct EnqueuedItem {
    pub item: String,
    pub metadata: TrackMetaData,
}

#[derive(Debug, Clone)]
pub enum EnqueueType {
    Track(EnqueuedItem),
    Playlist(EnqueuedItem),
}

#[derive(Debug, Clone)]
pub enum ProcessedQueueRemovalCondition {
    All,
    Duplicates,
    Indices(Vec<usize>),
    FromUser(UserId),
}

#[derive(Debug, Clone)]
pub enum PlayStateChange {
    Resume,
    Pause,
    ToggleLoop,
}

#[derive(Debug, Clone)]
pub struct QueueItem {
    pub index: usize,
    pub track_metadata: Metadata,
    pub extra_metadata: TrackMetaData,
}

#[derive(Debug, Clone)]
pub struct QueueItemFull {
    pub index: usize,
    pub track_metadata: Metadata,
    pub extra_metadata: TrackMetaDataFull,
}

impl QueueItem {
    pub async fn fetch_data(self, ctx: &Ctx, guild_id: &GuildId) -> anyhow::Result<QueueItemFull> {
        Ok(QueueItemFull {
            index: self.index,
            track_metadata: self.track_metadata,
            extra_metadata: self.extra_metadata.fetch_data(ctx, guild_id).await?,
        })
    }
}
