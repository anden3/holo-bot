use std::{fmt::Display, ops::Deref, str::FromStr};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, CommaSeparator, NoneAsEmptyString, StringWithSeparator};

use crate::errors::Error;

#[derive(Debug, Clone, PartialEq, Eq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
/// The ID of a video.
pub struct VideoId(pub(crate) String);

impl Display for VideoId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Deref for VideoId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<&str> for VideoId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl From<String> for VideoId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl FromStr for VideoId {
    type Err = Error;

    #[allow(clippy::unwrap_in_result)]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        #[allow(clippy::expect_used)]
        let regex =
            Regex::new(r"[0-9A-Za-z_-]{10}[048AEIMQUYcgkosw]").expect("Video ID regex broke.");

        Ok(regex
            .find(s)
            .ok_or_else(|| Error::InvalidVideoId(s.to_owned()))?
            .as_str()
            .into())
    }
}

#[serde_as]
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Room {
    #[serde(rename = "Nick")]
    pub name: String,

    #[serde(alias = "EntryPass", alias = "Entrypass")]
    pub needs_password: bool,

    #[serde(rename = "ExtShare")]
    #[serde(default)]
    pub allows_external_sharing: bool,

    #[serde(rename = "Empty")]
    pub is_empty: bool,

    #[serde(rename = "StreamLink")]
    #[serde_as(as = "NoneAsEmptyString")]
    pub stream: Option<VideoId>,

    #[serde(rename = "Tags")]
    #[serde(default)]
    #[serde_as(as = "StringWithSeparator::<CommaSeparator, String>")]
    pub tags: Vec<String>,
}

/* #[derive(Debug, Clone, Deserialize)]
#[serde(tag = "flag", content = "content")]
pub enum EventData {
    #[serde(alias = "connect")]
    Connect(String),
    #[serde(alias = "insert", alias = "new")]
    Insert {
        #[serde(rename = "_id")]
        id: String,
        #[serde(rename = "Stext")]
        text: String,
        #[serde(rename = "Stime")]
        time: u64,
    },
    #[serde(alias = "delete")]
    Delete(String),
    #[serde(alias = "update", alias = "change")]
    Update {
        #[serde(rename = "_id")]
        id: String,
        #[serde(rename = "Stext")]
        text: String,
        #[serde(rename = "Stime")]
        time: u64,
    },
} */

#[derive(Debug, Clone)]
pub enum RoomUpdate {
    Added(VideoId),
    Removed(VideoId),
    Changed(VideoId, VideoId),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "flag", content = "content")]
pub enum EventData<T> {
    #[serde(alias = "connect")]
    Connect(String),
    #[serde(alias = "insert", alias = "new")]
    Insert(T),
    #[serde(alias = "delete")]
    Delete(String),
    #[serde(alias = "update", alias = "change")]
    Update(T),
}

#[derive(Debug, Clone, Deserialize)]
pub struct RoomEvent {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "Stext")]
    pub text: String,
    #[serde(rename = "Stime")]
    pub time: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ArchiveEvent(pub serde_json::Value);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(untagged)]
pub enum DataOrEmptyObject<T> {
    Some(T),
    None {},
}

impl<T> From<DataOrEmptyObject<T>> for Option<T> {
    fn from(empty_option: DataOrEmptyObject<T>) -> Option<T> {
        match empty_option {
            DataOrEmptyObject::Some(option) => Some(option),
            DataOrEmptyObject::None {} => None,
        }
    }
}

impl<T> From<Option<T>> for DataOrEmptyObject<T> {
    fn from(option: Option<T>) -> Self {
        match option {
            Some(option) => Self::Some(option),
            None {} => Self::None {},
        }
    }
}

impl<T> DataOrEmptyObject<T> {
    pub fn into_option(self) -> Option<T> {
        self.into()
    }
    pub fn as_option(&self) -> Option<&T> {
        match self {
            Self::Some(option) => Some(option),
            Self::None {} => None,
        }
    }
}
