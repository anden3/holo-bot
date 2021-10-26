use std::{convert::Infallible, str::FromStr};

use chrono::Duration;
use holodex::model::id::VideoId;
use isolang::Language as LanguageCode;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, CommaSeparator, DurationMilliSeconds, StringWithSeparator};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TranslatorId(String);

impl std::fmt::Display for TranslatorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<TranslatorId> for String {
    fn from(id: TranslatorId) -> Self {
        id.0
    }
}

impl From<String> for TranslatorId {
    fn from(id: String) -> Self {
        TranslatorId(id)
    }
}

impl FromStr for TranslatorId {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(TranslatorId(s.to_string()))
    }
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize, Eq)]
#[serde(rename_all(serialize = "PascalCase"))]
pub struct Translation {
    pub id: u64,
    pub video_id: VideoId,
    pub translator_id: TranslatorId,
    #[serde(rename = "language_code")]
    pub language: Language,
    pub translated_text: String,
    #[serde_as(as = "DurationMilliSeconds<i64>")]
    pub start: Duration,
    #[serde_as(as = "Option<DurationMilliSeconds<i64>>")]
    pub end: Option<Duration>,
}

impl std::hash::Hash for Translation {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl PartialEq for Translation {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl PartialOrd for Translation {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.id.cmp(&other.id))
    }
}

impl Ord for Translation {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

impl std::fmt::Display for Translation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} [{}]: {}",
            match self.end {
                Some(end) => format!("[{}-{}]", self.start, end),
                None => format!("[{}]", self.start),
            },
            self.translator_id,
            self.language,
            self.translated_text
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "PascalCase"))]
pub struct Translator {
    #[serde(rename = "UserId")]
    pub id: TranslatorId,
    #[serde(rename = "DisplayName")]
    pub name: String,
    #[serde(rename = "ProfilePictureUrl")]
    pub picture: Option<String>,
    #[serde(rename = "Type")]
    pub translator_type: TranslatorType,
    #[serde(default)]
    pub languages: Vec<Language>,
}

#[non_exhaustive]
#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all(serialize = "PascalCase"))]
pub enum TranslatorType {
    Registered,
    Verified,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq)]
#[serde(rename_all(serialize = "PascalCase"))]
pub struct Language {
    pub code: LanguageCode,
    pub name: String,
    pub native_name: String,
}

impl PartialEq for Language {
    fn eq(&self, other: &Self) -> bool {
        self.code == other.code
    }
}

impl PartialOrd for Language {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.name.cmp(&other.name))
    }
}

impl Ord for Language {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

impl std::hash::Hash for Language {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.code.hash(state);
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationFilter {
    #[serde_as(as = "DurationMilliSeconds<i64>")]
    #[serde(skip_serializing_if = "duration_is_minus_one")]
    pub since: Duration,

    #[serde_as(as = "StringWithSeparator::<CommaSeparator, TranslatorId>")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub require: Vec<TranslatorId>,

    #[serde_as(as = "StringWithSeparator::<CommaSeparator, TranslatorId>")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<TranslatorId>,
}

impl Default for TranslationFilter {
    fn default() -> Self {
        Self {
            since: Duration::milliseconds(-1),
            require: Vec::new(),
            exclude: Vec::new(),
        }
    }
}

fn duration_is_minus_one(duration: &Duration) -> bool {
    duration.num_milliseconds() == -1
}
