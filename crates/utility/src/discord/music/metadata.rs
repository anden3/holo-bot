use serenity::utils::Colour;

use super::prelude::*;

#[derive(Debug, Clone)]
pub struct TrackMetaData {
    pub added_by: UserId,
    pub added_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct TrackMetaDataFull {
    pub added_by: UserId,
    pub colour: Colour,
    pub added_by_name: String,
    pub added_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub(crate) struct UserData {
    pub(crate) name: String,
    pub(crate) colour: Colour,
}

#[derive(Debug, Clone)]
pub struct ExtractedMetaData {
    pub title: String,
    pub uploader: String,
    pub duration: Duration,
    pub thumbnail: Option<String>,
}

impl From<ytextract::Video> for ExtractedMetaData {
    fn from(video: ytextract::Video) -> Self {
        Self {
            title: video.title().to_owned(),
            uploader: video.channel().name().to_owned(),
            duration: video.duration(),
            thumbnail: video
                .thumbnails()
                .first()
                .map(|t| t.url.as_str().to_owned()),
        }
    }
}

impl From<ytextract::playlist::Video> for ExtractedMetaData {
    fn from(video: ytextract::playlist::Video) -> Self {
        Self {
            title: video.title().to_owned(),
            uploader: video.channel().name().to_owned(),
            duration: video.length(),
            thumbnail: video
                .thumbnails()
                .first()
                .map(|t| t.url.as_str().to_owned()),
        }
    }
}
