#![allow(dead_code)]

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{self, Deserialize, Serialize};
use serde_enum_str::{Deserialize_enum_str, Serialize_enum_str};
use serde_with::{serde_as, CommaSeparator, DisplayFromStr, StringWithSeparator};
use strum_macros::ToString;

use utility::{functions::is_default, streams::VideoStatus};

#[serde_as]
#[derive(Serialize, Debug, Clone)]
pub(crate) struct ApiLiveOptions {
    pub channel_id: Option<String>,
    pub id: Option<String>,
    #[serde_as(as = "StringWithSeparator::<CommaSeparator, ExtraVideoInfo>")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<ExtraVideoInfo>,
    #[serde_as(as = "StringWithSeparator::<CommaSeparator, VideoLanguage>")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lang: Vec<VideoLanguage>,
    pub limit: u32,
    pub max_upcoming_hours: u32,
    pub mentioned_channel_id: Option<String>,
    pub offset: i32,
    pub order: VideoOrder,
    pub org: Option<Organisation>,
    #[serde_as(as = "DisplayFromStr")]
    #[serde(skip_serializing_if = "is_default")]
    pub paginated: bool,
    pub sort: SortBy,
    #[serde_as(as = "StringWithSeparator::<CommaSeparator, VideoStatus>")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub status: Vec<VideoStatus>,
    pub topic: Option<String>,
    #[serde(rename = "type")]
    pub video_type: VideoType,
}

#[non_exhaustive]
#[allow(dead_code)]
#[derive(Serialize, Debug, ToString, Copy, Clone)]
#[serde(rename_all(serialize = "snake_case"))]
#[strum(serialize_all = "snake_case")]
pub(crate) enum ExtraVideoInfo {
    Clips,
    Refers,
    Sources,
    Simulcasts,
    Mentions,
    Description,
    LiveInfo,
    ChannelStats,
    Songs,
}

#[non_exhaustive]
#[allow(dead_code)]
#[derive(Serialize, Debug, ToString, Copy, Clone)]
#[serde(rename_all(serialize = "lowercase"))]
#[strum(serialize_all = "lowercase")]
pub(crate) enum VideoLanguage {
    All,
    EN,
    JP,
}

#[allow(dead_code)]
#[derive(Serialize, Debug, Copy, Clone)]
pub(crate) enum VideoOrder {
    #[serde(rename = "asc")]
    Ascending,
    #[serde(rename = "desc")]
    Descending,
}

#[non_exhaustive]
#[allow(dead_code)]
#[derive(Deserialize_enum_str, Serialize_enum_str, Debug, PartialEq, Clone)]
#[serde(rename_all = "PascalCase")]
pub enum Organisation {
    Hololive,
    Nijisanji,
    Independents,
    #[serde(other)]
    Other(String),
}

#[non_exhaustive]
#[allow(dead_code)]
#[derive(Serialize, Debug, Copy, Clone)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SortBy {
    Id,
    Title,
    Type,
    TopicId,
    PublishedAt,
    AvailableAt,
    Duration,
    Status,
    StartScheduled,
    StartActual,
    EndActual,
    LiveViewers,
    Description,
    #[serde(rename = "songcount")]
    SongCount,
    ChannelId,
}

#[non_exhaustive]
#[allow(dead_code)]
#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VideoType {
    Stream,
    Clip,
}

impl Default for ApiLiveOptions {
    fn default() -> Self {
        Self {
            channel_id: None,
            id: None,
            include: vec![ExtraVideoInfo::LiveInfo],
            lang: vec![VideoLanguage::All],
            limit: 9999,
            max_upcoming_hours: 672,
            mentioned_channel_id: None,
            offset: 0,
            order: VideoOrder::Descending,
            org: Some(Organisation::Hololive),
            paginated: true,
            sort: SortBy::AvailableAt,
            status: Vec::new(),
            topic: None,
            video_type: VideoType::Stream,
        }
    }
}

#[serde_as]
#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub(crate) enum ApiLiveResponse {
    Videos(Vec<Video>),
    Page {
        #[serde_as(as = "DisplayFromStr")]
        total: i32,
        #[serde(default)]
        items: Vec<Video>,
    },
}

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct Video {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub video_type: VideoType,
    #[serde(default)]
    pub topic_id: Option<String>,
    #[serde(with = "utility::serializers::opt_utc_datetime")]
    #[serde(default)]
    pub published_at: Option<DateTime<Utc>>,
    #[serde(with = "utility::serializers::utc_datetime")]
    pub available_at: DateTime<Utc>,
    pub duration: u32,
    pub status: VideoStatus,
    #[serde(flatten)]
    pub live_info: VideoLiveInfo,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub live_tl_count: Option<HashMap<String, u32>>,
    #[serde(rename = "songcount")]
    #[serde(default)]
    pub song_count: Option<u32>,
    #[serde(alias = "channel_id")]
    pub channel: VideoChannel,
}

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct ChannelMin {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub english_name: Option<String>,
    #[serde(rename = "type")]
    pub channel_type: ChannelType,
    pub photo: String,
    #[serde(default)]
    pub org: Option<Organisation>,
}

#[serde_as]
#[derive(Deserialize, Debug, Clone)]
pub(crate) struct Channel {
    pub id: String,
    pub name: String,
    pub description: String,
    pub inactive: bool,

    #[serde(rename = "type")]
    pub channel_type: ChannelType,

    #[serde(default)]
    pub lang: Option<String>,
    #[serde(default)]
    pub english_name: Option<String>,
    #[serde(default)]
    pub org: Option<Organisation>,
    #[serde(default)]
    pub suborg: Option<String>,
    #[serde(default)]
    pub photo: Option<String>,
    #[serde(default)]
    pub banner: Option<String>,
    #[serde(default)]
    pub twitter: Option<String>,

    #[serde_as(as = "Option<DisplayFromStr>")]
    #[serde(default)]
    pub video_count: Option<u32>,
    #[serde_as(as = "Option<DisplayFromStr>")]
    #[serde(default)]
    pub subscriber_count: Option<u32>,
    #[serde_as(as = "Option<DisplayFromStr>")]
    #[serde(default)]
    pub view_count: Option<u32>,
    #[serde_as(as = "Option<DisplayFromStr>")]
    #[serde(default)]
    pub clip_count: Option<u32>,

    #[serde(with = "utility::serializers::utc_datetime")]
    pub published_at: DateTime<Utc>,
    #[serde(with = "utility::serializers::opt_utc_datetime")]
    pub crawled_at: Option<DateTime<Utc>>,
    #[serde(with = "utility::serializers::opt_utc_datetime")]
    pub comments_crawled_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub(crate) enum VideoChannel {
    Id(String),
    Data(ChannelMin),
}

impl VideoChannel {
    pub fn get_id(&self) -> &String {
        match self {
            Self::Id(id) => id,
            Self::Data(d) => &d.id,
        }
    }
}

#[non_exhaustive]
#[allow(dead_code)]
#[derive(Deserialize, Debug, Copy, Clone)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ChannelType {
    VTuber,
    Subber,
}

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct VideoWithChannel {
    #[serde(flatten)]
    pub video: Video,
    #[serde(flatten)]
    pub channel: ChannelMin,
}

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct VideoFull {
    #[serde(flatten)]
    pub video: Video,

    #[serde(default)]
    pub clips: Vec<VideoWithChannel>,
    #[serde(default)]
    pub sources: Vec<VideoWithChannel>,
    #[serde(default)]
    pub refers: Vec<VideoWithChannel>,
    #[serde(default)]
    pub simulcasts: Vec<VideoWithChannel>,
    #[serde(default)]
    pub mentions: Vec<ChannelMin>,
    #[serde(default)]
    pub songs: Option<u32>,
}

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct VideoLiveInfo {
    #[serde(default)]
    #[serde(with = "utility::serializers::opt_utc_datetime")]
    pub start_scheduled: Option<DateTime<Utc>>,
    #[serde(default)]
    #[serde(with = "utility::serializers::opt_utc_datetime")]
    pub start_actual: Option<DateTime<Utc>>,
    #[serde(default)]
    #[serde(with = "utility::serializers::opt_utc_datetime")]
    pub end_actual: Option<DateTime<Utc>>,
    #[serde(default)]
    pub live_viewers: Option<u32>,
}

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct Comment {
    pub comment_key: String,
    pub video_id: String,
    pub message: String,
}

#[derive(Debug)]
pub(crate) enum VideoUpdate {
    Scheduled(String),
    Started(String),
    Ended(String),
    Unscheduled(String),
    Renamed {
        id: String,
        new_name: String,
    },
    Rescheduled {
        id: String,
        new_start: DateTime<Utc>,
    },
}
