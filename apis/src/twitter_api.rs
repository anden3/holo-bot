use anyhow::{anyhow, Context};
use backoff::ExponentialBackoff;
use bytes::Bytes;
use chrono::prelude::*;
use futures::{Stream, StreamExt};
use log::{debug, error, info, warn};
use reqwest::{Client, Error, Response};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use tokio::sync::mpsc::Sender;

use super::{discord_api::DiscordMessageData, translation_api::TranslationApi};
use utility::{config, extensions::VecExt, here};

pub struct TwitterApi {}

impl TwitterApi {
    pub async fn start(config: config::Config, notifier_sender: Sender<DiscordMessageData>) {
        tokio::spawn(async move {
            match Self::run(config, notifier_sender).await {
                Ok(_) => (),
                Err(e) => {
                    error!("{:?}", e);
                }
            }
        });
    }

    async fn run(
        config: config::Config,
        notifier_sender: Sender<DiscordMessageData>,
    ) -> anyhow::Result<()> {
        use reqwest::header;

        let formatted_token = format!("Bearer {}", &config.twitter_token);
        let mut headers = header::HeaderMap::new();

        let mut auth_val = header::HeaderValue::from_str(&formatted_token).context(here!())?;
        auth_val.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, auth_val);

        let client = reqwest::ClientBuilder::new()
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION"),
            ))
            .default_headers(headers)
            .build()
            .context(here!())?;

        Self::setup_rules(&client, &config.users).await?;

        let translator = match TranslationApi::new(&config) {
            Ok(api) => api,
            Err(e) => {
                return Err(anyhow!(e));
            }
        };

        let mut stream = Self::connect(&client).await?;

        while let Some(item) = stream.next().await {
            if let Ok(message) = item {
                match Self::parse_message(message, &config.users, &translator).await {
                    Ok(Some(discord_message)) => {
                        notifier_sender
                            .send(discord_message)
                            .await
                            .context(here!())?;
                    }
                    Ok(None) => (),
                    Err(e) => error!("{:?}", e),
                }
            }
        }

        Ok(())
    }

    async fn connect(client: &Client) -> anyhow::Result<impl Stream<Item = Result<Bytes, Error>>> {
        let backoff_config = ExponentialBackoff {
            randomization_factor: 0.0,
            multiplier: 2.0,
            ..ExponentialBackoff::default()
        };

        Ok(backoff::future::retry(backoff_config, || async {
            let response = client
                .get("https://api.twitter.com/2/tweets/search/stream")
                .query(&[
                    ("expansions", "attachments.media_keys,referenced_tweets.id"),
                    ("media.fields", "url"),
                    (
                        "tweet.fields",
                        "author_id,created_at,lang,in_reply_to_user_id,referenced_tweets",
                    ),
                ])
                .send()
                .await
                .map_err(|e| {
                    warn!("{}", e.to_string());
                    anyhow!(e).context(here!())
                })?;

            Self::check_rate_limit(&response).map_err(|e| {
                warn!("{}", e.to_string());
                anyhow!(e).context(here!())
            })?;
            response.error_for_status_ref().map_err(|e| {
                warn!("{}", e.to_string());
                anyhow!(e).context(here!())
            })?;

            Ok(response.bytes_stream())
        })
        .await
        .context(here!())?)
    }

    #[allow(clippy::too_many_lines)]
    async fn parse_message(
        message: Bytes,
        users: &[config::User],
        translator: &TranslationApi,
    ) -> anyhow::Result<Option<DiscordMessageData>> {
        if message == "\r\n" {
            return Ok(None);
        }

        let message: Tweet = serde_json::from_slice(&message).context(here!())?;

        // Find who made the tweet.
        let user = users
            .iter()
            .find(|u| u.twitter_id == message.data.author_id)
            .ok_or({
                anyhow!(
                    "Could not find user with twitter ID: {}",
                    message.data.author_id
                )
            })
            .context(here!())?;

        // Check for schedule keyword.
        if let Some(keyword) = &user.schedule_keyword {
            if let Some(includes) = &message.includes {
                if !includes.media.is_empty()
                    && message
                        .data
                        .text
                        .to_lowercase()
                        .contains(&keyword.to_lowercase())
                {
                    info!("New schedule update from {}.", user.display_name);

                    let schedule_image = match &includes.media[..] {
                        [media, ..] => match media.url.as_ref() {
                            Some(url) => url.to_string(),
                            None => {
                                return Err(
                                    anyhow!("Detected schedule image had no URL.").context(here!())
                                )
                            }
                        },
                        [] => {
                            return Err(anyhow!("Detected schedule post didn't include image!")
                                .context(here!()))
                        }
                    };

                    return Ok(Some(DiscordMessageData::ScheduleUpdate(ScheduleUpdate {
                        twitter_id: user.twitter_id,
                        tweet_text: message.data.text,
                        schedule_image,
                        tweet_link: format!(
                            "https://twitter.com/{}/status/{}",
                            user.twitter_handle, message.data.id
                        ),
                        timestamp: message.data.created_at,
                    })));
                }
            }
        }

        // Check if we're replying to another talent.
        let mut replied_to: Option<HoloTweetReference> = None;

        if !message.data.referenced_tweets.is_empty() {
            let reference = message
                .data
                .referenced_tweets
                .first()
                .ok_or_else(|| anyhow!("Can't reach tweet reference!").context(here!()))?;

            let replied_to_user = match reference.reply_type.as_str() {
                "replied_to" => message
                    .data
                    .in_reply_to_user_id
                    .ok_or_else(|| {
                        anyhow!("Tweet reply didn't contain a in_reply_to_user_id field.")
                    })
                    .context(here!())?,
                "quoted" => {
                    message
                        .includes
                        .as_ref()
                        .ok_or_else(|| anyhow!("Quoted reply didn't include any expansion object."))
                        .context(here!())?
                        .tweets
                        .iter()
                        .find(|t| t.id == reference.id)
                        .ok_or_else(|| anyhow!("Couldn't find referenced tweet in expanded field."))
                        .context(here!())?
                        .author_id
                }
                _ => {
                    return Err(
                        anyhow!("Unknown reply type: {}", reference.reply_type).context(here!())
                    )
                }
            };

            if users.iter().any(|u| replied_to_user == u.twitter_id) {
                replied_to = Some(HoloTweetReference {
                    user: replied_to_user,
                    tweet: reference.id,
                });
            } else {
                // If tweet is replying to someone who is not a Hololive talent, don't show the tweet.
                return Ok(None);
            }
        }

        // Add attachments if they exist.
        let mut media = Vec::new();

        if let Some(includes) = message.includes {
            for m in includes.media {
                match m.url {
                    Some(url) if m.media_type == "photo" => media.push(url),
                    Some(_) | None => (),
                }
            }
        }

        // Check if translation is necessary.
        let mut translation: Option<String> = None;

        if let Some(lang) = message.data.lang {
            match lang.as_str() {
                "in" | "id" | "de" | "ja" | "jp" => {
                    if let Ok(tl) = translator
                        .get_translator_for_lang(&lang)
                        .translate(&message.data.text, &lang)
                        .await
                    {
                        translation = Some(tl);
                    }
                }
                _ => (),
            }
        }

        info!("New tweet from {}.", user.display_name);

        let tweet = HoloTweet {
            id: message.data.id,
            user: user.clone(),
            text: message.data.text,
            link: format!(
                "https://twitter.com/{}/status/{}",
                user.twitter_id, message.data.id
            ),
            timestamp: message.data.created_at,
            media,
            translation,
            replied_to,
        };

        Ok(Some(DiscordMessageData::Tweet(tweet)))
    }

    async fn setup_rules(client: &Client, users: &[config::User]) -> anyhow::Result<()> {
        let mut rules = vec![];
        let mut current_rule = String::with_capacity(512);
        let mut i = 0;

        while i < users.len() {
            let user = &users
                .get(i)
                .ok_or_else(|| anyhow!("Couldn't get user!"))
                .context(here!())?;
            let new_segment;

            if current_rule.is_empty() {
                current_rule += "-is:retweet (";
                new_segment = format!("from:{}", user.twitter_id)
            } else {
                new_segment = format!(" OR from:{}", user.twitter_id)
            }

            if current_rule.len() + new_segment.len() < 511 {
                current_rule += &new_segment;
                i += 1;
            } else {
                rules.push(Rule {
                    value: current_rule.clone() + ")",
                    tag: format!("Hololive Talents {}", rules.len() + 1),
                });

                current_rule.clear();
            }
        }

        if !current_rule.is_empty() {
            rules.push(Rule {
                value: current_rule.clone() + ")",
                tag: format!("Hololive Talents {}", rules.len() + 1),
            });
        }

        let existing_rules = Self::get_rules(client).await?;

        if rules == existing_rules {
            return Ok(());
        }

        Self::delete_rules(client, existing_rules).await?;

        let update: RuleUpdate = RuleUpdate {
            add: rules,
            delete: IdList { ids: Vec::new() },
        };

        let response = client
            .post("https://api.twitter.com/2/tweets/search/stream/rules")
            .json(&update)
            .send()
            .await
            .context(here!())?;

        Self::check_rate_limit(&response)?;
        let response = Self::validate_response::<RuleUpdateResponse>(response).await?;

        if let Some(meta) = response.meta {
            if meta.summary.invalid > 0 {
                panic!(
                    "{} invalid rules found! Rules are {:#?}.",
                    meta.summary.invalid, update.add
                );
            }
        }

        Ok(())
    }

    async fn get_rules(client: &Client) -> anyhow::Result<Vec<RemoteRule>> {
        let response = client
            .get("https://api.twitter.com/2/tweets/search/stream/rules")
            .send()
            .await
            .context(here!())?;

        Self::check_rate_limit(&response)?;

        let mut response = Self::validate_response::<RuleRequestResponse>(response).await?;
        response.data.sort_unstable_by_key_ref(|r| &r.tag);

        Ok(response.data)
    }

    async fn delete_rules(client: &Client, rules: Vec<RemoteRule>) -> anyhow::Result<()> {
        let request = RuleUpdate {
            add: Vec::new(),
            delete: IdList {
                ids: rules.iter().map(|r| r.id).collect(),
            },
        };

        if rules.is_empty() {
            return Ok(());
        }

        let response = client
            .post("https://api.twitter.com/2/tweets/search/stream/rules")
            .json(&request)
            .send()
            .await
            .context(here!())?;

        Self::check_rate_limit(&response)?;
        let response = Self::validate_response::<RuleUpdateResponse>(response).await?;

        if let Some(meta) = response.meta {
            if meta.summary.deleted != rules.len() {
                panic!(
                    "Wrong number of rules deleted! {} instead of {}!",
                    meta.summary.deleted,
                    rules.len()
                );
            }
        }

        Ok(())
    }

    fn check_rate_limit(response: &Response) -> anyhow::Result<()> {
        use chrono_humanize::{Accuracy, Tense};

        let headers = response.headers();

        let remaining = headers
            .get("x-rate-limit-remaining")
            .ok_or_else(|| anyhow!("x-rate-limit-remaining header not found in response!"))?
            .to_str()?
            .parse::<i32>()?;

        let limit = headers
            .get("x-rate-limit-limit")
            .ok_or_else(|| anyhow!("x-rate-limit-limit header not found in response!"))?
            .to_str()?
            .parse::<i32>()?;

        let reset = headers
            .get("x-rate-limit-reset")
            .ok_or_else(|| anyhow!("x-rate-limit-reset header not found in response!"))?
            .to_str()?;

        // Convert timestamp to local time.
        let reset = NaiveDateTime::from_timestamp(reset.parse::<i64>()?, 0);
        let reset_utc: DateTime<Utc> = DateTime::from_utc(reset, Utc);
        let reset_local_time: DateTime<Local> = DateTime::from(reset_utc);

        // Get duration until reset happens.
        let local_time = Local::now();
        let time_until_reset = reset_local_time - local_time;
        let humanized_time = chrono_humanize::HumanTime::from(time_until_reset);

        debug!(
            "{}/{} requests made (Resets {})",
            limit - remaining,
            limit,
            humanized_time.to_text_en(Accuracy::Precise, Tense::Future)
        );

        if remaining <= 0 {
            Err(anyhow!("Rate limit reached.").context(here!()))
        } else {
            Ok(())
        }
    }

    async fn validate_response<T>(response: Response) -> anyhow::Result<T>
    where
        T: DeserializeOwned + CanContainError,
    {
        if let Err(error_code) = (&response).error_for_status_ref().context(here!()) {
            let response_bytes = response.bytes().await.context(here!())?;
            let deserializer = &mut serde_json::Deserializer::from_slice(&response_bytes);
            let response: Result<T, _> = serde_path_to_error::deserialize(deserializer);

            match response {
                Ok(response) => {
                    if let Some(err_msg) = response.get_error() {
                        error!("{:#?}", err_msg);
                    }

                    Err(error_code)
                }
                Err(e) => {
                    error!(
                        "Deserialization error at '{}' in {}.",
                        e.path().to_string(),
                        here!()
                    );
                    error!(
                        "Data:\r\n{:?}",
                        std::str::from_utf8(&response_bytes).context(here!())?
                    );
                    Err(e.into())
                }
            }
        } else {
            let response_bytes = response.bytes().await.context(here!())?;
            let deserializer = &mut serde_json::Deserializer::from_slice(&response_bytes);
            let response: Result<T, _> = serde_path_to_error::deserialize(deserializer);

            match response {
                Ok(response) => Ok(response),
                Err(e) => {
                    error!(
                        "Deserialization error at '{}' in {}.",
                        e.path().to_string(),
                        here!()
                    );
                    error!(
                        "Data:\r\n{:?}",
                        std::str::from_utf8(&response_bytes).context(here!())?
                    );
                    Err(e.into())
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct ScheduleUpdate {
    pub twitter_id: u64,
    pub tweet_text: String,
    pub schedule_image: String,
    pub tweet_link: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug)]
pub struct HoloTweet {
    pub id: u64,
    pub user: config::User,
    pub text: String,
    pub link: String,
    pub timestamp: DateTime<Utc>,
    pub media: Vec<String>,
    pub translation: Option<String>,
    pub replied_to: Option<HoloTweetReference>,
}

#[derive(Debug)]
pub struct HoloTweetReference {
    pub user: u64,
    pub tweet: u64,
}

trait CanContainError {
    fn get_error(&self) -> Option<&ApiError>;
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct ApiError {
    #[serde(rename = "type")]
    error_type: String,
    title: String,

    value: Option<String>,
    detail: Option<String>,
    reason: Option<String>,
    client_id: Option<String>,
    registration_url: Option<String>,
    required_enrollment: Option<String>,
}

#[derive(Debug, Serialize)]
struct RuleUpdate {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    add: Vec<Rule>,
    #[serde(skip_serializing_if = "IdList::is_empty")]
    delete: IdList,
}

#[derive(Serialize, Debug)]
struct Rule {
    value: String,
    #[serde(default)]
    tag: String,
}

#[derive(Debug, Serialize)]
struct IdList {
    ids: Vec<u64>,
}

impl IdList {
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

#[derive(Deserialize, Debug)]
struct Tweet {
    data: TweetInfo,
    includes: Option<Expansions>,
    matching_rules: Vec<MatchingRule>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct RuleRequestResponse {
    #[serde(default = "Vec::new")]
    data: Vec<RemoteRule>,
    meta: RuleRequestResponseMeta,

    #[serde(flatten)]
    error: Option<ApiError>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct RuleUpdateResponse {
    data: Option<Vec<RemoteRule>>,
    meta: Option<RuleUpdateResponseMeta>,

    #[serde(flatten)]
    error: Option<ApiError>,
}

#[serde_as]
#[derive(Deserialize, Debug)]
struct TweetInfo {
    attachments: Option<TweetAttachments>,
    #[serde_as(as = "DisplayFromStr")]
    author_id: u64,
    #[serde_as(as = "DisplayFromStr")]
    id: u64,
    text: String,
    #[serde(with = "utility::serializers::utc_datetime")]
    created_at: DateTime<Utc>,
    lang: Option<String>,
    #[serde_as(as = "Option<DisplayFromStr>")]
    #[serde(default)]
    in_reply_to_user_id: Option<u64>,
    #[serde(default = "Vec::new")]
    referenced_tweets: Vec<TweetReference>,
}

#[derive(Deserialize, Debug)]
struct TweetAttachments {
    media_keys: Vec<String>,
}

#[serde_as]
#[derive(Deserialize, Debug)]
struct TweetReference {
    #[serde(rename = "type")]
    reply_type: String,
    #[serde_as(as = "DisplayFromStr")]
    id: u64,
}

#[derive(Deserialize, Debug)]
struct Expansions {
    #[serde(default = "Vec::new")]
    media: Vec<MediaInfo>,
    #[serde(default = "Vec::new")]
    tweets: Vec<TweetInfo>,
}

#[derive(Deserialize, Debug)]
struct MediaInfo {
    media_key: String,
    #[serde(rename = "type")]
    media_type: String,
    url: Option<String>,
}

#[derive(Deserialize, Debug)]
struct MatchingRule {
    id: u64,
    tag: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct RuleRequestResponseMeta {
    #[serde(with = "utility::serializers::utc_datetime")]
    sent: DateTime<Utc>,
}

#[allow(dead_code)]
#[serde_as]
#[derive(Deserialize, Debug)]
struct RemoteRule {
    #[serde_as(as = "DisplayFromStr")]
    id: u64,
    value: String,
    #[serde(default)]
    tag: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct RuleUpdateResponseMeta {
    #[serde(with = "utility::serializers::utc_datetime")]
    sent: DateTime<Utc>,
    summary: RuleUpdateResponseMetaSummary,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct RuleUpdateResponseMetaSummary {
    #[serde(default)]
    created: usize,
    #[serde(default)]
    not_created: usize,
    #[serde(default)]
    deleted: usize,
    #[serde(default)]
    not_deleted: usize,
    #[serde(default)]
    valid: usize,
    #[serde(default)]
    invalid: usize,
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
