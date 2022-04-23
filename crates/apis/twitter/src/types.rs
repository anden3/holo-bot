#![allow(dead_code)]

pub mod id;

use std::{collections::HashMap, ops::Range};

use chrono::{DateTime, Utc};
use isolang::Language;
use serde::{Deserialize, Serialize};
use serde_with::{
    rust::StringWithSeparator, serde_as, CommaSeparator, DefaultOnNull, DurationMilliSeconds,
    FromInto, TryFromInto,
};
use smartstring::alias::String as SmartString;
use strum::Display;

use id::*;

/* #[cfg(feature = "translation")]
use crate::translation_api::TranslationApi; */

#[cfg(feature = "academic_research_track")]
use bounded_integer::BoundedU8;

use crate::errors::Error;

pub(crate) trait CanContainError {
    fn get_error(&self) -> Option<&ApiError>;
}

#[non_exhaustive]
#[derive(Debug, Copy, Clone, Serialize, Display)]
pub enum RequestedExpansion {
    /// Includes a user object representing the Tweet’s author.
    #[serde(rename = "author_id")]
    #[strum(serialize = "author_id")]
    AuthorId,
    /// Includes a Tweet object that this Tweet is referencing
    /// (either as a Retweet, Quoted Tweet, or reply)
    #[serde(rename = "referenced_tweets.id")]
    #[strum(serialize = "referenced_tweets.id")]
    ReferencedTweet,
    /// Includes a user object for the author of the referenced Tweet.
    #[serde(rename = "referenced_tweets.id.author_id")]
    #[strum(serialize = "referenced_tweets.id.author_id")]
    ReferencedTweetAuthor,
    /// Includes a user object representing the Tweet author a requested Tweet is a reply to.
    #[serde(rename = "in_reply_to_user_id")]
    #[strum(serialize = "in_reply_to_user_id")]
    InReplyTo,
    /// Includes a media object representing the images, videos, GIFs included in the Tweet.
    #[serde(rename = "attachments.media_keys")]
    #[strum(serialize = "attachments.media_keys")]
    AttachedMedia,
    /// Includes a poll object containing metadata for the poll included in the Tweet.
    #[serde(rename = "attachments.poll_ids")]
    #[strum(serialize = "attachments.poll_ids")]
    AttachedPoll,
    /// Includes a place object containing metadata for the location tagged in the Tweet.
    #[serde(rename = "geo.place_id")]
    #[strum(serialize = "geo.place_id")]
    TaggedLocation,
    /// Includes a user object for the user mentioned in the Tweet.
    #[serde(rename = "entities.mentions.username")]
    #[strum(serialize = "entities.mentions.username")]
    MentionedUser,
    #[serde(rename = "pinned_tweet_id")]
    #[strum(serialize = "pinned_tweet_id")]
    PinnedTweet,
}

#[non_exhaustive]
#[derive(Debug, Copy, Clone, Serialize, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum TweetField {
    Attachments,
    AuthorId,
    ContextAnnotations,
    ConversationId,
    CreatedAt,
    Entities,
    Geo,
    InReplyToUserId,
    Lang,
    NonPublicMetrics,
    OrganicMetrics,
    PossiblySensitive,
    PromotedMetrics,
    ReferencedTweets,
    ReplySettings,
    Source,
    Withheld,
}

#[non_exhaustive]
#[derive(Debug, Copy, Clone, Serialize, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum UserField {
    CreatedAt,
    Description,
    Entities,
    Location,
    PinnedTweetId,
    ProfileImageUrl,
    Protected,
    PublicMetrics,
    Url,
    Verified,
    Withheld,
}

#[non_exhaustive]
#[derive(Debug, Copy, Clone, Serialize, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum MediaField {
    #[serde(rename = "duration_ms")]
    #[strum(serialize = "duration_ms")]
    Duration,
    Height,
    NonPublicMetrics,
    OrganicMetrics,
    PreviewImageUrl,
    PromotedMetrics,
    PublicMetrics,
    Width,
    AltText,
    Url,
}

#[non_exhaustive]
#[derive(Debug, Copy, Clone, Serialize, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum PollField {
    #[serde(rename = "duration_minutes")]
    #[strum(serialize = "duration_minutes")]
    Duration,
    EndDatetime,
    VotingStatus,
}

#[non_exhaustive]
#[derive(Debug, Copy, Clone, Serialize, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum PlaceField {
    ContainedWithin,
    Country,
    CountryCode,
    Geo,
    #[serde(rename = "geo.coordinates")]
    #[strum(serialize = "geo.coordinates")]
    GeoCoordinates,
    Name,
    PlaceType,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct StreamParameters {
    #[cfg(feature = "academic_research_track")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backfill_minutes: Option<BoundedU8<1, 5>>,

    #[serde(with = "StringWithSeparator::<CommaSeparator>")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub expansions: Vec<RequestedExpansion>,

    #[serde(with = "StringWithSeparator::<CommaSeparator>")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(rename = "media.fields")]
    pub media_fields: Vec<MediaField>,

    #[serde(with = "StringWithSeparator::<CommaSeparator>")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(rename = "place.fields")]
    pub place_fields: Vec<PlaceField>,

    #[serde(with = "StringWithSeparator::<CommaSeparator>")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(rename = "poll.fields")]
    pub poll_fields: Vec<PollField>,

    #[serde(with = "StringWithSeparator::<CommaSeparator>")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(rename = "tweet.fields")]
    pub tweet_fields: Vec<TweetField>,

    #[serde(with = "StringWithSeparator::<CommaSeparator>")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(rename = "place.fields")]
    pub user_fields: Vec<UserField>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RecentTweetCountParameters {
    pub query: RuleString,
    #[serde(skip_serializing_if = "crate::util::is_default")]
    pub granularity: TweetCountGranularity,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub since_id: Option<TweetId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until_id: Option<TweetId>,
}

#[cfg(feature = "academic_research_track")]
#[derive(Debug, Clone, Serialize, Default)]
pub struct AllTweetCountParameters {
    pub query: RuleString,
    #[serde(skip_serializing_if = "crate::util::is_default")]
    pub granularity: TweetCountGranularity,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub since_id: Option<TweetId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until_id: Option<TweetId>,

    pub next_token: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TweetCountGranularity {
    Minute,
    Hour,
    Day,
}

impl Default for TweetCountGranularity {
    fn default() -> Self {
        Self::Hour
    }
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct ApiError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub title: String,
    #[serde(default)]
    pub errors: Vec<ApiSubError>,

    pub value: Option<String>,
    pub detail: Option<String>,
    pub reason: Option<String>,
    pub client_id: Option<String>,
    pub disconnect_type: Option<String>,
    pub registration_url: Option<String>,
    pub required_enrollment: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct ApiSubError {
    pub message: String,
    #[serde(default)]
    pub parameters: HashMap<String, Vec<serde_json::Value>>,
}

#[derive(Debug, Serialize, Default)]
pub(crate) struct RuleUpdate {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub add: Vec<Rule>,
    #[serde(skip_serializing_if = "IdList::is_empty")]
    pub delete: IdList,
}

impl RuleUpdate {
    pub fn add(rules: Vec<Rule>) -> Self {
        Self {
            add: rules,
            ..Default::default()
        }
    }

    pub fn remove(ids: Vec<RuleId>) -> Self {
        Self {
            delete: IdList { ids },
            ..Default::default()
        }
    }
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Rule {
    pub value: RuleString,
    #[serde(default)]
    pub tag: String,
}

impl PartialOrd for Rule {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Rule {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.tag.cmp(&other.tag) {
            std::cmp::Ordering::Equal => (),
            order => return order,
        }

        self.value.cmp(&other.value)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct RuleString(pub String);

impl TryFrom<String> for RuleString {
    type Error = Error;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        #[cfg(feature = "academic_research_track")]
        const RULE_LIMIT: usize = 1024;
        #[cfg(not(feature = "academic_research_track"))]
        const RULE_LIMIT: usize = 512;

        if s.len() > RULE_LIMIT {
            return Err(Error::RuleLengthExceeded {
                length: s.len(),
                rule: s,
                limit: RULE_LIMIT,
            });
        }

        Ok(Self(s))
    }
}

impl From<RuleString> for String {
    fn from(s: RuleString) -> Self {
        s.0
    }
}

impl std::ops::Deref for RuleString {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Serialize, Default)]
pub(crate) struct IdList {
    pub ids: Vec<RuleId>,
}

impl IdList {
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum TweetOrError {
    Tweet(Tweet),
    Error { errors: Vec<ApiError> },
}

#[derive(Deserialize, Debug)]
pub struct Tweet {
    pub data: TweetInfo,
    #[serde(default)]
    pub includes: Option<Expansions>,
    pub matching_rules: Vec<MatchingRule>,
}

impl Tweet {
    pub fn attached_photos(&self) -> impl Iterator<Item = &str> {
        self.includes
            .iter()
            .flat_map(|i| i.media.iter())
            .filter_map(|m| match &m.url {
                Some(url) if m.media_type == MediaType::Photo => Some(url.as_str()),
                Some(_) | None => None,
            })
    }

    /* #[cfg(feature = "translation")]
    pub async fn translate(&self, translator: &TranslationApi) -> Option<String> {
        let lang = &self.data.lang.as_deref()?;

        match translator
            .get_translator_for_lang(lang)?
            .translate(&self.data.text, lang)
            .await
        {
            Ok(tl) => Some(tl),
            Err(e) => {
                error!("{:?}", e);
                None
            }
        }
    } */
}

#[serde_as]
#[derive(Deserialize, Debug)]
pub struct TweetInfo {
    pub id: TweetId,
    pub text: String,

    #[serde(default)]
    pub attachments: Option<TweetAttachments>,
    #[serde(default)]
    pub author_id: Option<UserId>,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub conversation_id: Option<TweetId>,
    #[serde(default)]
    pub in_reply_to_user_id: Option<UserId>,
    #[serde(default)]
    pub referenced_tweets: Vec<TweetReference>,
    #[serde(default)]
    pub geo: Option<Geo>,

    #[cfg(feature = "entities")]
    #[serde(default)]
    pub context_annotations: Vec<ContextAnnotation>,
    #[cfg(feature = "entities")]
    #[serde(default)]
    pub entities: Entities,

    #[serde(default)]
    pub withheld: Option<WithheldInfo>,
    #[serde(default)]
    pub possibly_sensitive: Option<bool>,
    #[serde(default)]
    #[serde_as(as = "Option<TryFromInto<TwitterLanguage>>")]
    pub lang: Option<Language>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub reply_settings: Option<ReplySettings>,

    #[cfg(feature = "metrics")]
    #[serde(default)]
    pub public_metrics: Option<PublicMetrics>,
    #[cfg(feature = "metrics")]
    #[serde(default)]
    pub organic_metrics: Option<TweetMetrics>,
    #[cfg(feature = "metrics")]
    #[serde(default)]
    pub promoted_metrics: Option<TweetMetrics>,
}

#[derive(Deserialize, Serialize)]
pub struct TwitterLanguage(pub String);

impl TryFrom<TwitterLanguage> for Language {
    type Error = String;

    fn try_from(lang: TwitterLanguage) -> Result<Self, Self::Error> {
        match lang.0.as_str() {
            "in" => Ok(Language::Ind),
            "und" => Ok(Language::Und),
            l => Language::from_639_1(l).ok_or(format!("Could not parse language tag: {}", l)),
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct TweetAttachments {
    #[serde(default = "Vec::new")]
    pub media_keys: Vec<SmartString>,

    #[serde(default = "Vec::new")]
    pub poll_ids: Vec<PollId>,
}

#[derive(Deserialize, Debug)]
pub struct TweetReference {
    #[serde(rename = "type")]
    pub reply_type: TweetReferenceType,
    pub id: TweetId,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum TweetReferenceType {
    Retweeted,
    Quoted,
    RepliedTo,
}

#[derive(Deserialize, Debug)]
pub struct Geo {
    #[serde(default)]
    pub coordinates: Option<GeoCoordinates>,
    #[serde(default)]
    pub place_id: Option<PlaceId>,
}

#[serde_as]
#[derive(Deserialize, Debug)]
pub struct GeoCoordinates {
    #[serde(rename = "type")]
    pub coordinate_type: GeoCoordinateType,
    #[serde_as(as = "DefaultOnNull")]
    pub coordinates: Vec<(f64, f64)>,
}

#[derive(Deserialize, Debug)]
pub enum GeoCoordinateType {
    Point,
}

#[cfg(feature = "entities")]
#[derive(Deserialize, Debug)]
pub struct ContextAnnotation {
    pub domain: ContextAnnotationDomain,
    #[serde(default)]
    pub entity: Option<ContextAnnotationEntity>,
}

#[cfg(feature = "entities")]
#[derive(Deserialize, Debug)]
pub struct ContextAnnotationDomain {
    pub id: ContextAnnotationDomainId,
    pub name: String,
    pub description: String,
}

#[cfg(feature = "entities")]
#[derive(Deserialize, Debug)]
pub struct ContextAnnotationEntity {
    pub id: ContextAnnotationEntityId,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[cfg(feature = "entities")]
#[derive(Debug)]
pub enum Entity {
    Annotation {
        range: Range<u16>,
        probability: f32,
        entity_type: String,
        normalized_text: String,
    },
    Hashtag {
        range: Range<u16>,
        tag: String,
    },
    Mention {
        range: Range<u16>,
        username: String,
    },
    Url {
        range: Range<u16>,
        url: String,
        expanded_url: String,
        display_url: String,
        unwound_url: Option<String>,
    },
}

impl Entity {
    pub fn embed_link(&self, text: &mut String) {
        tracing::debug!(?self, "Embedding link in tweet text: {text:#?}");

        match self {
            Entity::Hashtag { tag, .. } => {
                *text = text.replace(
                    &format!("#{tag}"),
                    &format!("[#{tag}](https://twitter.com/hashtag/{tag})"),
                )
            }
            Entity::Mention { username, .. } => {
                *text = text.replace(
                    &format!("@{username}"),
                    &format!("[@{username}](https://twitter.com/{username})"),
                )
            }
            Entity::Url {
                url,
                expanded_url,
                display_url,
                ..
            } => {
                *text = text.replace(url, &format!("[{display_url}]({expanded_url})"));
            }
            _ => (),
        }
    }
}

#[cfg(feature = "entities")]
#[derive(Deserialize, Debug, Default)]
#[serde(from = "EntitiesRaw")]
pub struct Entities(Vec<Entity>);

impl Entities {
    pub fn iter(&self) -> impl Iterator<Item = &Entity> {
        self.0.iter()
    }
}

impl IntoIterator for Entities {
    type Item = Entity;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl From<EntitiesRaw> for Entities {
    fn from(raw_entities: EntitiesRaw) -> Self {
        let mut entities = Vec::new();

        for hashtag in raw_entities.hashtags {
            entities.push(Entity::Hashtag {
                range: hashtag.range,
                tag: hashtag.tag,
            });
        }

        for mention in raw_entities.mentions {
            entities.push(Entity::Mention {
                range: mention.range,
                username: mention.username,
            });
        }

        for url in raw_entities.urls {
            entities.push(Entity::Url {
                range: url.range,
                url: url.url,
                expanded_url: url.expanded_url,
                display_url: url.display_url,
                unwound_url: url.unwound_url,
            });
        }

        for annotation in raw_entities.annotations {
            entities.push(Entity::Annotation {
                range: annotation.range,
                probability: annotation.probability,
                entity_type: annotation.entity_type,
                normalized_text: annotation.normalized_text,
            });
        }

        Entities(entities)
    }
}

impl std::fmt::Display for Entity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Entity::Annotation {
                entity_type,
                normalized_text,
                ..
            } => write!(f, "{entity_type} - {normalized_text}"),
            Entity::Hashtag { tag, .. } => write!(f, "#{tag}"),
            Entity::Mention { username, .. } => write!(f, "@{username}"),
            Entity::Url { expanded_url, .. } => write!(f, "{expanded_url}"),
        }
    }
}

#[cfg(feature = "entities")]
#[derive(Deserialize, Debug)]
struct EntitiesRaw {
    #[serde(default)]
    pub annotations: Vec<EntityAnnotation>,
    #[serde(default)]
    pub urls: Vec<EntityUrl>,
    #[serde(default)]
    pub hashtags: Vec<EntityHashtag>,
    #[serde(default)]
    pub mentions: Vec<EntityMention>,
    #[serde(default)]
    pub cashtags: Vec<EntityCashtag>,
}

#[cfg(feature = "entities")]
#[derive(Deserialize, Debug)]
struct EntityAnnotation {
    #[serde(flatten)]
    pub range: Range<u16>,
    pub probability: f32,
    #[serde(rename = "type")]
    pub entity_type: String,
    pub normalized_text: String,
}

impl std::fmt::Display for EntityAnnotation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} - {}", self.entity_type, self.normalized_text)
    }
}

#[cfg(feature = "entities")]
#[derive(Deserialize, Debug)]
struct EntityUrl {
    #[serde(flatten)]
    pub range: Range<u16>,
    pub url: String,
    pub expanded_url: String,
    pub display_url: String,
    pub unwound_url: Option<String>,
}

impl std::fmt::Display for EntityUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.expanded_url)
    }
}

#[cfg(feature = "entities")]
#[derive(Deserialize, Debug, Clone)]
struct EntityHashtag {
    #[serde(flatten)]
    pub range: Range<u16>,
    pub tag: String,
}

impl std::fmt::Display for EntityHashtag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.tag)
    }
}

#[cfg(feature = "entities")]
#[derive(Deserialize, Debug)]
struct EntityMention {
    #[serde(flatten)]
    pub range: Range<u16>,
    #[serde(alias = "tag")]
    pub username: String,
}

impl std::fmt::Display for EntityMention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "@{}", self.username)
    }
}

#[cfg(feature = "entities")]
#[derive(Deserialize, Debug)]
struct EntityCashtag {
    #[serde(flatten)]
    pub range: Range<u16>,
    pub tag: String,
}

impl std::fmt::Display for EntityCashtag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "${}", self.tag)
    }
}

#[derive(Deserialize, Debug)]
pub struct WithheldInfo {
    #[serde(default)]
    pub copyright: bool,
    #[serde(default)]
    pub country_codes: Vec<SmartString>,
    pub scope: WithheldScope,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum WithheldScope {
    Tweet,
    User,
}

#[cfg(feature = "metrics")]
#[derive(Deserialize, Debug)]
pub struct PublicMetrics {
    #[serde(flatten)]
    pub metrics: TweetMetrics,
    pub quote_count: u64,
}

#[cfg(feature = "metrics")]
#[derive(Deserialize, Debug)]
pub struct TweetMetrics {
    pub retweet_count: u64,
    pub reply_count: u64,
    pub like_count: u64,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub enum ReplySettings {
    Everyone,
    MentionedUsers,
    Following,
}

#[derive(Deserialize, Debug, Default)]
pub struct Expansions {
    #[serde(default)]
    pub media: Vec<Media>,
    #[serde(default)]
    pub tweets: Vec<TweetInfo>,
    #[serde(default)]
    pub users: Vec<User>,
    #[serde(default)]
    pub places: Vec<Place>,
    #[serde(default)]
    pub polls: Vec<Poll>,
}

#[serde_as]
#[derive(Deserialize, Debug)]
pub struct Media {
    pub media_key: SmartString,
    #[serde(rename = "type")]
    pub media_type: MediaType,
    #[serde(default)]
    #[serde(rename = "duration_ms")]
    #[serde_as(as = "Option<DurationMilliSeconds<i64>>")]
    pub duration: Option<chrono::Duration>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub alt_text: Option<String>,

    #[cfg(feature = "metrics")]
    #[serde(default)]
    pub non_public_metrics: Option<MediaMetrics>,
    #[cfg(feature = "metrics")]
    #[serde(default)]
    pub organic_metrics: Option<MediaEngagementMetrics>,
    #[cfg(feature = "metrics")]
    #[serde(default)]
    pub promoted_metrics: Option<MediaEngagementMetrics>,
    #[cfg(feature = "metrics")]
    #[serde(default)]
    pub public_metrics: Option<ViewCount>,
}

#[cfg(feature = "metrics")]
#[derive(Debug, Deserialize)]
pub struct MediaMetrics {
    pub playback_0_count: u64,
    pub playback_25_count: u64,
    pub playback_50_count: u64,
    pub playback_75_count: u64,
    pub playback_100_count: u64,
}

#[cfg(feature = "metrics")]
#[derive(Debug, Deserialize)]
pub struct MediaEngagementMetrics {
    #[serde(flatten)]
    pub metrics: MediaMetrics,
    pub view_count: u64,
}

#[cfg(feature = "metrics")]
#[derive(Deserialize, Debug)]
pub struct ViewCount {
    pub view_count: u64,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MediaType {
    AnimatedGif,
    Photo,
    Video,
}

#[serde_as]
#[derive(Deserialize, Debug)]
pub struct Poll {
    pub id: PollId,
    pub options: Vec<PollOption>,
    #[serde(rename = "duration_minutes")]
    #[serde_as(as = "Option<FromInto<crate::util::DurationMinutes>>")]
    pub duration: Option<chrono::Duration>,
    #[serde(rename = "end_datetime")]
    pub ends_at: DateTime<Utc>,
    pub voting_status: PollVotingStatus,
}

#[derive(Deserialize, Debug)]
pub struct PollOption {
    pub position: u8,
    pub label: String,
    pub votes: u64,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum PollVotingStatus {
    #[serde(alias = "active")]
    Open,
    Closed,
}

#[derive(Deserialize, Debug)]
pub struct Place {
    pub id: PlaceId,
    pub full_name: String,

    #[serde(default)]
    pub contained_within: Vec<PlaceId>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub country_code: Option<SmartString>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub place_type: Option<String>,
    #[serde(default)]
    pub geo: Option<GeoFull>,
}

#[derive(Deserialize, Debug)]
pub struct GeoFull {
    #[serde(rename = "type")]
    pub geo_type: GeoType,
    #[serde(default)]
    pub bbox: Vec<f32>,
}

#[derive(Deserialize, Debug)]
pub enum GeoType {
    Feature,
    FeatureCollection,
    Point,
    MultiPoint,
    LineString,
    MultiLineString,
    Polygon,
    MultiPolygon,
    GeometryCollection,
}

#[derive(Deserialize, Debug)]
pub struct User {
    pub id: UserId,
    pub name: String,
    pub username: String,

    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub pinned_tweet_id: Option<TweetId>,
    #[serde(default)]
    pub profile_image_url: Option<String>,
    #[serde(default)]
    pub protected: Option<bool>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub verified: Option<bool>,

    #[cfg(feature = "metrics")]
    #[serde(default)]
    pub public_metrics: Option<UserMetrics>,

    #[cfg(feature = "entitites")]
    #[serde(default)]
    pub entities: Option<UserEntities>,

    #[serde(default)]
    pub withheld: Option<serde_json::Value>,
}

#[cfg(feature = "metrics")]
#[derive(Deserialize, Debug)]
pub struct UserMetrics {
    pub followers_count: u64,
    pub following_count: u64,
    pub tweet_count: u64,
    pub listed_count: u64,
}

#[cfg(feature = "entities")]
#[derive(Deserialize, Debug)]
pub struct UserEntities {
    #[serde(default)]
    pub description: Entities,
    #[serde(default)]
    pub url: Entities,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub(crate) struct RuleRequestResponse {
    #[serde(default = "Vec::new")]
    pub data: Vec<ActiveRule>,
    pub meta: RuleRequestResponseMeta,

    #[serde(flatten)]
    pub error: Option<ApiError>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub(crate) struct RuleUpdateResponse {
    pub data: Option<Vec<ActiveRule>>,
    pub meta: Option<RuleUpdateResponseMeta>,

    #[serde(flatten)]
    pub error: Option<ApiError>,
}

#[derive(Deserialize, Debug)]
pub struct MatchingRule {
    pub id: RuleId,
    pub tag: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub(crate) struct RuleRequestResponseMeta {
    pub sent: DateTime<Utc>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug, Clone, Eq)]
pub struct ActiveRule {
    pub id: RuleId,
    pub value: RuleString,
    #[serde(default)]
    pub tag: String,
}

impl PartialOrd for ActiveRule {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ActiveRule {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.tag.cmp(&other.tag) {
            std::cmp::Ordering::Equal => (),
            order => return order,
        }

        match self.value.cmp(&other.value) {
            std::cmp::Ordering::Equal => (),
            order => return order,
        }

        self.id.cmp(&other.id)
    }
}

impl PartialEq for ActiveRule {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl std::hash::Hash for ActiveRule {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl From<ActiveRule> for Rule {
    fn from(active_rule: ActiveRule) -> Self {
        Rule {
            value: active_rule.value,
            tag: active_rule.tag,
        }
    }
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub(crate) struct RuleUpdateResponseMeta {
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

impl PartialEq<ActiveRule> for Rule {
    fn eq(&self, other: &ActiveRule) -> bool {
        self.value == other.value && self.tag == other.tag
    }
}

impl PartialEq<Rule> for ActiveRule {
    fn eq(&self, other: &Rule) -> bool {
        self.value == other.value && self.tag == other.tag
    }
}

impl<S1, S2> TryFrom<(S1, S2)> for Rule
where
    S1: TryInto<RuleString>,
    S2: Into<String>,
    Error: From<<S1 as TryInto<RuleString>>::Error>,
{
    type Error = Error;

    fn try_from((value, tag): (S1, S2)) -> Result<Self, Self::Error> {
        Ok(Rule {
            value: value.try_into()?,
            tag: tag.into(),
        })
    }
}

pub enum ProductTrack {
    Standard,
    AcademicResearch,
}

impl Default for ProductTrack {
    fn default() -> Self {
        ProductTrack::Standard
    }
}

#[derive(Deserialize, Debug)]
pub struct TweetCounts {
    #[serde(rename = "data")]
    pub segments: Vec<TweetCountSegment>,
    pub meta: TweetCountMeta,
}

#[derive(Debug, Deserialize)]
pub struct TweetCountSegment {
    pub tweet_count: u64,
    #[serde(flatten)]
    pub range: Range<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct TweetCountMeta {
    pub total_tweet_count: u64,
    #[cfg(feature = "academic_research_track")]
    pub next_token: Option<String>,
}
