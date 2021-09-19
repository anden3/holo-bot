use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};

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
pub(crate) struct Tweet {
    pub data: TweetInfo,
    pub includes: Option<Expansions>,
    pub matching_rules: Vec<MatchingRule>,
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

#[derive(Deserialize, Debug)]
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
