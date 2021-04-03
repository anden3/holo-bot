use std::time::Duration;

use bytes::Bytes;
use chrono::prelude::*;
use futures::{Stream, StreamExt};
use log::{debug, error, info, warn};
use reqwest::{Client, Error, Response};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use tokio::{sync::mpsc::Sender, time::sleep};

use super::config;
use super::discord_api::DiscordMessageData;
use super::extensions::VecExt;
use super::translation_api::TranslationAPI;

pub struct TwitterAPI {}

impl TwitterAPI {
    pub async fn start(config: config::Config, notifier_sender: Sender<DiscordMessageData>) {
        tokio::spawn(async move {
            TwitterAPI::run(config, notifier_sender).await.unwrap();
        });
    }

    async fn run(
        config: config::Config,
        notifier_sender: Sender<DiscordMessageData>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use reqwest::header;

        let formatted_token = format!("Bearer {}", &config.twitter_token);
        let mut headers = header::HeaderMap::new();

        let mut auth_val = header::HeaderValue::from_str(&formatted_token)?;
        auth_val.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, auth_val);

        let client = reqwest::ClientBuilder::new()
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION"),
            ))
            .default_headers(headers)
            .build()?;

        TwitterAPI::setup_rules(&client, &config.users)
            .await
            .map_err(|e| {
                error!("{}", e);
                e
            })
            .unwrap();

        let translator = TranslationAPI::new(&config);

        let mut stream = TwitterAPI::connect(&client).await?;

        while let Some(item) = stream.next().await {
            if let Ok(message) = item {
                match TwitterAPI::parse_message(message, &config.users, &translator).await {
                    Ok(Some(discord_message)) => {
                        notifier_sender.send(discord_message).await.unwrap();
                    }
                    Ok(None) => (),
                    Err(e) => error!("{}", e),
                }
            }
        }

        Ok(())
    }

    async fn connect(
        client: &Client,
    ) -> Result<impl Stream<Item = Result<Bytes, Error>>, Box<dyn std::error::Error>> {
        let mut backoff_time: Duration = Duration::from_secs(5);

        loop {
            let response = client
                .get("https://api.twitter.com/2/tweets/search/stream")
                .query(&[
                    ("expansions", "attachments.media_keys"),
                    ("media.fields", "url"),
                    (
                        "tweet.fields",
                        "author_id,created_at,lang,in_reply_to_user_id,referenced_tweets",
                    ),
                ])
                .send()
                .await
                .unwrap();

            if let Err(e) = TwitterAPI::check_rate_limit(&response) {
                error!("{}", e);
                sleep(backoff_time).await;
                backoff_time *= 2;
                continue;
            }

            if let Err(e) = response.error_for_status_ref() {
                error!("{}", e);
                sleep(backoff_time).await;
                backoff_time *= 2;
                continue;
            }

            return Ok(response.bytes_stream());
        }
    }

    async fn parse_message(
        message: Bytes,
        users: &Vec<config::User>,
        translator: &TranslationAPI,
    ) -> Result<Option<DiscordMessageData>, String> {
        if message == "\r\n" {
            return Ok(None);
        }

        let message: Tweet = serde_json::from_slice(&message).map_err(|e| format!("{}", e))?;

        // Find who made the tweet.
        let user = users
            .iter()
            .find(|u| u.twitter_id == message.data.author_id)
            .ok_or({
                format!(
                    "Could not find user with twitter ID: {}",
                    message.data.author_id
                )
            })?;

        info!("New tweet from {}.", user.display_name);

        if let Some(keyword) = &user.schedule_keyword {
            if let Some(includes) = &message.includes {
                if !includes.media.is_empty()
                    && message
                        .data
                        .text
                        .to_lowercase()
                        .contains(&keyword.to_lowercase())
                {
                    return Ok(Some(DiscordMessageData::ScheduleUpdate(ScheduleUpdate {
                        twitter_id: user.twitter_id,
                        tweet_text: message.data.text,
                        schedule_image: includes.media[0].url.as_ref().unwrap().to_string(),
                        tweet_link: format!(
                            "https://twitter.com/{}/status/{}",
                            user.twitter_handle, message.data.id
                        ),
                        timestamp: message.data.created_at,
                    })));
                }
            }
        }

        // Add attachments if they exist.
        let mut media = Vec::new();

        if let Some(includes) = message.includes {
            for m in includes.media {
                if m.media_type == "photo" {
                    media.push(m.url.unwrap());
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

        // Check if we're replying to another talent.
        let mut replied_to: Option<HoloTweetReference> = None;

        if let Some(replied_to_user) = message.data.in_reply_to_usr_id {
            if users.iter().any(|u| replied_to_user == u.twitter_id) {
                let ref_tweets = &message.data.referenced_tweets;

                if ref_tweets.len() == 0 {
                    warn!("Tweet reply doesn't have any referenced tweets! Link: https://twitter.com/{}/status/{}", user.twitter_id, message.data.id);
                } else if ref_tweets.len() > 1 {
                    warn!("Tweet reply has more than two referenced tweets! Link: https://twitter.com/{}/status/{}", user.twitter_id, message.data.id);
                } else {
                    let reference = ref_tweets.first().unwrap();

                    if reference.reply_type == "replied_to" {
                        replied_to = Some(HoloTweetReference {
                            user: replied_to_user,
                            tweet: reference.id,
                        });
                    }
                }
            }
        }

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

    async fn setup_rules(
        client: &Client,
        users: &Vec<config::User>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut rules = vec![];
        let mut current_rule = String::with_capacity(512);
        let mut i = 0;

        while i < users.len() {
            let user = &users[i];
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

        let existing_rules = TwitterAPI::get_rules(&client).await?;

        if rules == existing_rules {
            return Ok(());
        }

        TwitterAPI::delete_rules(&client, existing_rules).await?;

        let update: RuleUpdate = RuleUpdate {
            add: rules,
            delete: IDList { ids: Vec::new() },
        };

        let response = client
            .post("https://api.twitter.com/2/tweets/search/stream/rules")
            .json(&update)
            .send()
            .await?;

        TwitterAPI::check_rate_limit(&response)?;
        let response = TwitterAPI::validate_response::<RuleUpdateResponse>(response).await?;

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

    async fn get_rules(client: &Client) -> Result<Vec<RemoteRule>, Box<dyn std::error::Error>> {
        let response = client
            .get("https://api.twitter.com/2/tweets/search/stream/rules")
            .send()
            .await?;

        TwitterAPI::check_rate_limit(&response)?;

        let mut response = TwitterAPI::validate_response::<RuleRequestResponse>(response).await?;
        response.data.sort_unstable_by_key_ref(|r| &r.tag);

        Ok(response.data)
    }

    async fn delete_rules(
        client: &Client,
        rules: Vec<RemoteRule>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let request = RuleUpdate {
            add: Vec::new(),
            delete: IDList {
                ids: rules.iter().map(|r| r.id).collect(),
            },
        };

        if rules.len() == 0 {
            return Ok(());
        }

        let response = client
            .post("https://api.twitter.com/2/tweets/search/stream/rules")
            .json(&request)
            .send()
            .await?;

        TwitterAPI::check_rate_limit(&response)?;
        let response = TwitterAPI::validate_response::<RuleUpdateResponse>(response).await?;

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

    fn check_rate_limit(response: &Response) -> Result<(), std::io::Error> {
        use chrono_humanize::{Accuracy, Tense};
        use std::io;

        let headers = response.headers();

        let remaining = headers
            .get("x-rate-limit-remaining")
            .unwrap()
            .to_str()
            .unwrap()
            .parse::<i32>()
            .unwrap();

        let limit = headers
            .get("x-rate-limit-limit")
            .unwrap()
            .to_str()
            .unwrap()
            .parse::<i32>()
            .unwrap();

        let reset = headers.get("x-rate-limit-reset").unwrap().to_str().unwrap();

        // Convert timestamp to local time.
        let reset = NaiveDateTime::from_timestamp(reset.parse::<i64>().unwrap(), 0);
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
            Err(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                "Rate limit reached.",
            ))
        } else {
            Ok(())
        }
    }

    async fn validate_response<T>(response: Response) -> Result<T, Error>
    where
        T: DeserializeOwned,
        T: CanContainError,
    {
        if let Err(error_code) = (&response).error_for_status_ref() {
            let response: T = response
                .json()
                .await
                .expect("Deserialization of Error failed.");

            /* let response_bytes = response.bytes().await.unwrap();
            println!("{}", std::str::from_utf8(&response_bytes).unwrap());
            let response: T = serde_json::from_slice(&response_bytes).unwrap(); */

            if let Some(err_msg) = response.get_error() {
                println!("Error: {:#?}", err_msg);
            }

            return Err(error_code);
        } else {
            let response: T = response.json().await.unwrap();

            /* let response_bytes = response.bytes().await.unwrap();
            println!("{}", std::str::from_utf8(&response_bytes).unwrap());
            let response: T = serde_json::from_slice(&response_bytes).unwrap(); */

            Ok(response)
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
    fn get_error(&self) -> Option<&APIError>;
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct APIError {
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
    #[serde(skip_serializing_if = "IDList::is_empty")]
    delete: IDList,
}

#[derive(Serialize, Debug)]
struct Rule {
    value: String,
    #[serde(default)]
    tag: String,
}

#[derive(Debug, Serialize)]
struct IDList {
    ids: Vec<u64>,
}

impl IDList {
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
    error: Option<APIError>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct RuleUpdateResponse {
    meta: Option<RuleUpdateResponseMeta>,
    data: Option<Vec<RemoteRule>>,

    #[serde(flatten)]
    error: Option<APIError>,
}

#[serde_as]
#[derive(Deserialize, Debug)]
struct TweetInfo {
    attachments: Option<TweetAttachments>,
    // #[serde(with = "super::serializers::string_to_number")]
    #[serde_as(as = "DisplayFromStr")]
    author_id: u64,
    // #[serde(with = "super::serializers::string_to_number")]
    #[serde_as(as = "DisplayFromStr")]
    id: u64,
    text: String,
    #[serde(with = "super::serializers::utc_datetime")]
    created_at: DateTime<Utc>,
    lang: Option<String>,
    #[serde_as(as = "Option<DisplayFromStr>")]
    #[serde(default)]
    in_reply_to_usr_id: Option<u64>,
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
    media: Vec<MediaInfo>,
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
    #[serde(with = "super::serializers::utc_datetime")]
    sent: DateTime<Utc>,
}

#[allow(dead_code)]
#[serde_as]
#[derive(Deserialize, Debug)]
struct RemoteRule {
    // #[serde(with = "super::serializers::string_to_number")]
    #[serde_as(as = "DisplayFromStr")]
    id: u64,
    value: String,
    #[serde(default)]
    tag: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
struct RuleUpdateResponseMeta {
    #[serde(with = "super::serializers::utc_datetime")]
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
    fn get_error(&self) -> Option<&APIError> {
        if let Some(error) = &self.error {
            Some(error)
        } else {
            None
        }
    }
}

impl CanContainError for RuleUpdateResponse {
    fn get_error(&self) -> Option<&APIError> {
        if let Some(error) = &self.error {
            Some(error)
        } else {
            None
        }
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
