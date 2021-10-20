use songbird::input::Metadata;

use super::{
    metadata::{ExtractedMetaData, TrackMetaData},
    prelude::*,
};

use crate::{discord::FetchDiscordData, regex};

#[derive(Debug, Clone)]
pub struct EnqueuedItem {
    pub item: String,
    pub metadata: TrackMetaData,
    pub extracted_metadata: Option<ExtractedMetaData>,
}

impl EnqueuedItem {
    pub async fn fetch_metadata(
        &mut self,
        extractor: &ytextract::Client,
    ) -> Option<&ExtractedMetaData> {
        if self.extracted_metadata.is_some() {
            return self.extracted_metadata.as_ref();
        }

        if self.item.starts_with("ytsearch1:") {
            return None;
        }

        let video_id_rgx = regex!(r"[0-9A-Za-z_-]{10}[048AEIMQUYcgkosw]");

        if !video_id_rgx.is_match(&self.item) {
            return None;
        }

        let metadata = extractor
            .video(
                self.item
                    .parse()
                    .map_err(|e| error!(err = ?e, "Failed to parse video ID: {}", self.item))
                    .ok()?,
            )
            .await
            .map_err(|e| error!(err = ?e, "Failed to extract video metadata: {}", self.item))
            .ok()?;

        self.extracted_metadata = Some(ExtractedMetaData {
            title: metadata.title().to_owned(),
            uploader: metadata.channel().name().to_owned(),
            duration: metadata.duration(),
            thumbnail: metadata
                .thumbnails()
                .first()
                .map(|t| t.url.as_str().to_owned()),
        });

        self.extracted_metadata.as_ref()
    }
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
pub struct QueueItem<T> {
    pub index: usize,
    pub data: QueueItemData,
    pub extra_metadata: T,
}

#[derive(Debug, Clone)]
pub enum QueueItemData {
    BufferedTrack {
        metadata: Metadata,
    },
    UnbufferedTrack {
        url: String,
        metadata: Option<ExtractedMetaData>,
    },
    UnbufferedSearch {
        query: String,
    },
}

#[async_trait]
impl<T, K> FetchDiscordData<QueueItem<K>> for QueueItem<T>
where
    T: FetchDiscordData<K> + Send + Sync,
{
    async fn fetch_data(self, ctx: &Ctx, guild_id: &GuildId) -> anyhow::Result<QueueItem<K>> {
        Ok(QueueItem {
            index: self.index,
            data: self.data,
            extra_metadata: self.extra_metadata.fetch_data(ctx, guild_id).await?,
        })
    }
}
