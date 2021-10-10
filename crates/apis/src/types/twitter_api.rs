#![allow(dead_code)]

use anyhow::Context;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use tracing::{error, instrument, warn};
use utility::{config::Talent, here};

use crate::{
    translation_api::TranslationApi,
    twitter_api::{HoloTweetReference, ScheduleUpdate},
};

pub(crate) trait CanContainError {
    fn get_error(&self) -> Option<&ApiError>;
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub(crate) struct ApiError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub title: String,

    pub value: Option<String>,
    pub detail: Option<String>,
    pub reason: Option<String>,
    pub client_id: Option<String>,
    pub disconnect_type: Option<String>,
    pub registration_url: Option<String>,
    pub required_enrollment: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct RuleUpdate {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub add: Vec<Rule>,
    #[serde(skip_serializing_if = "IdList::is_empty")]
    pub delete: IdList,
}

#[derive(Serialize, Debug)]
pub(crate) struct Rule {
    pub value: String,
    #[serde(default)]
    pub tag: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct IdList {
    pub ids: Vec<u64>,
}

impl IdList {
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub(crate) enum TweetOrError {
    Tweet(Tweet),
    Error { errors: Vec<ApiError> },
}

#[derive(Deserialize, Debug)]
pub(crate) struct Tweet {
    pub data: TweetInfo,
    pub includes: Option<Expansions>,
    pub matching_rules: Vec<MatchingRule>,
}

impl Tweet {
    pub fn attached_photos(&self) -> impl Iterator<Item = &str> {
        self.includes
            .iter()
            .flat_map(|i| i.media.iter())
            .filter_map(|m| match &m.url {
                Some(url) if m.media_type == "photo" => Some(url.as_str()),
                Some(_) | None => None,
            })
    }

    pub fn talent_reply(&self, talents: &[&Talent]) -> Option<HoloTweetReference> {
        if self.data.referenced_tweets.is_empty() {
            return None;
        }
        let reference = self.data.referenced_tweets.first()?;

        let replied_to_user = match reference.reply_type.as_str() {
            "replied_to" => self.data.in_reply_to_user_id?,
            "quoted" => {
                self.includes
                    .as_ref()?
                    .tweets
                    .iter()
                    .find(|t| t.id == reference.id)?
                    .author_id
            }
            _ => {
                warn!(reply_type = ?reference.reply_type, "Unknown reply type");
                return None;
            }
        };

        if talents
            .iter()
            .any(|u| matches!(u.twitter_id, Some(id) if id == replied_to_user))
        {
            Some(HoloTweetReference {
                user: replied_to_user,
                tweet: reference.id,
            })
        } else {
            // If tweet is replying to someone who is not a Hololive talent, don't show the tweet.
            None
        }
    }

    #[instrument(skip(self, translator))]
    pub async fn translate(&self, translator: &TranslationApi) -> Option<String> {
        let lang = &self.data.lang.as_deref()?;

        match translator
            .get_translator_for_lang(lang)?
            .translate(&self.data.text, lang)
            .await
            .context(here!())
        {
            Ok(tl) => Some(tl),
            Err(e) => {
                error!("{:?}", e);
                None
            }
        }
    }

    pub fn schedule_update(&self, talent: &Talent) -> Option<ScheduleUpdate> {
        let keyword = talent.schedule_keyword.as_ref()?;
        let includes = self.includes.as_ref()?;

        if includes.media.is_empty()
            || !self
                .data
                .text
                .to_lowercase()
                .contains(&keyword.to_lowercase())
        {
            return None;
        }

        let schedule_image = match &includes.media[..] {
            [media, ..] => match media.url.as_ref() {
                Some(url) => url.to_string(),
                None => {
                    warn!("Detected schedule image had no URL.");
                    return None;
                }
            },
            [] => {
                warn!("Detected schedule post didn't include image!");
                return None;
            }
        };

        return Some(ScheduleUpdate {
            twitter_id: self.data.author_id,
            tweet_text: self.data.text.clone(),
            schedule_image,
            tweet_link: format!(
                "https://twitter.com/{}/status/{}",
                talent.twitter_handle.as_ref().unwrap(),
                self.data.id
            ),
            timestamp: self.data.created_at,
        });
    }
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub(crate) struct RuleRequestResponse {
    #[serde(default = "Vec::new")]
    pub data: Vec<RemoteRule>,
    pub meta: RuleRequestResponseMeta,

    #[serde(flatten)]
    pub error: Option<ApiError>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub(crate) struct RuleUpdateResponse {
    pub data: Option<Vec<RemoteRule>>,
    pub meta: Option<RuleUpdateResponseMeta>,

    #[serde(flatten)]
    pub error: Option<ApiError>,
}

#[serde_as]
#[derive(Deserialize, Debug)]
pub(crate) struct TweetInfo {
    pub attachments: Option<TweetAttachments>,
    #[serde_as(as = "DisplayFromStr")]
    pub author_id: u64,
    #[serde_as(as = "DisplayFromStr")]
    pub id: u64,
    pub text: String,
    #[serde(with = "utility::serializers::utc_datetime")]
    pub created_at: DateTime<Utc>,
    pub lang: Option<String>,
    #[serde_as(as = "Option<DisplayFromStr>")]
    #[serde(default)]
    pub in_reply_to_user_id: Option<u64>,
    #[serde(default = "Vec::new")]
    pub referenced_tweets: Vec<TweetReference>,
}

#[derive(Deserialize, Debug)]
pub(crate) struct TweetAttachments {
    #[serde(default = "Vec::new")]
    pub media_keys: Vec<String>,
}

#[serde_as]
#[derive(Deserialize, Debug)]
pub(crate) struct TweetReference {
    #[serde(rename = "type")]
    pub reply_type: String,
    #[serde_as(as = "DisplayFromStr")]
    pub id: u64,
}

#[derive(Deserialize, Debug, Default)]
pub(crate) struct Expansions {
    #[serde(default = "Vec::new")]
    pub media: Vec<MediaInfo>,
    #[serde(default = "Vec::new")]
    pub tweets: Vec<TweetInfo>,
}

#[derive(Deserialize, Debug)]
pub(crate) struct MediaInfo {
    pub media_key: String,
    #[serde(rename = "type")]
    pub media_type: String,
    pub url: Option<String>,
}

#[serde_as]
#[derive(Deserialize, Debug)]
pub(crate) struct MatchingRule {
    #[serde_as(as = "DisplayFromStr")]
    pub id: u64,
    pub tag: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub(crate) struct RuleRequestResponseMeta {
    #[serde(with = "utility::serializers::utc_datetime")]
    pub sent: DateTime<Utc>,
}

#[allow(dead_code)]
#[serde_as]
#[derive(Deserialize, Debug)]
pub(crate) struct RemoteRule {
    #[serde_as(as = "DisplayFromStr")]
    pub id: u64,
    pub value: String,
    #[serde(default)]
    pub tag: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub(crate) struct RuleUpdateResponseMeta {
    #[serde(with = "utility::serializers::utc_datetime")]
    pub sent: DateTime<Utc>,
    pub summary: RuleUpdateResponseMetaSummary,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub(crate) struct RuleUpdateResponseMetaSummary {
    #[serde(default)]
    pub created: usize,
    #[serde(default)]
    pub not_created: usize,
    #[serde(default)]
    pub deleted: usize,
    #[serde(default)]
    pub not_deleted: usize,
    #[serde(default)]
    pub valid: usize,
    #[serde(default)]
    pub invalid: usize,
}

impl CanContainError for RuleRequestResponse {
    fn get_error(&self) -> Option<&ApiError> {
        self.error.as_ref()
    }
}

impl CanContainError for RuleUpdateResponse {
    fn get_error(&self) -> Option<&ApiError> {
        self.error.as_ref()
    }
}

impl PartialEq<RemoteRule> for Rule {
    fn eq(&self, other: &RemoteRule) -> bool {
        self.value == other.value && self.tag == other.tag
    }
}

impl PartialEq<Rule> for RemoteRule {
    fn eq(&self, other: &Rule) -> bool {
        self.value == other.value && self.tag == other.tag
    }
}
