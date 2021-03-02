use bytes::Bytes;
use chrono::prelude::*;
use futures::{Stream, StreamExt};
use reqwest::{Client, Error, Response};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::sync::mpsc::Sender;

use super::config;
use super::discord_api::DiscordMessageData;

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

        let formatted_token = format!("Bearer {}", &config.bearer_token);
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

        let existing_rules = TwitterAPI::get_rules(&client).await?;

        TwitterAPI::delete_rules(&client, existing_rules).await?;
        TwitterAPI::setup_rules(&client, &config.users).await?;

        let mut stream = TwitterAPI::connect(&client).await?;

        while let Some(item) = stream.next().await {
            let response = item.unwrap();

            if response == "\r\n" {
                continue;
            }

            println!("{}", std::str::from_utf8(&response).unwrap());

            let response: Tweet =
                serde_json::from_slice(&response).expect("Deserialization of response failed.");

            if response.includes.media.is_empty() {
                continue;
            }

            println!("Response: {:#?}", response);

            notifier_sender
                .send(DiscordMessageData::ScheduleUpdate(ScheduleUpdate {
                    twitter_id: response.data.author_id,
                    tweet_text: response.data.text,
                    schedule_image: response.includes.media[0].url.as_ref().unwrap().to_string(),
                    tweet_link: format!(
                        "https://twitter.com/{}/status/{}",
                        response.data.author_id, response.data.id
                    ),
                    timestamp: response.data.created_at,
                }))
                .await
                .unwrap();
        }

        Ok(())
    }

    async fn get_rules(client: &Client) -> Result<Vec<RemoteRule>, Box<dyn std::error::Error>> {
        let response = client
            .get("https://api.twitter.com/2/tweets/search/stream/rules")
            .send()
            .await?;

        TwitterAPI::check_rate_limit(&response)?;
        let response = TwitterAPI::validate_response::<RuleRequestResponse>(response).await?;

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

    async fn setup_rules(
        client: &Client,
        _users: &Vec<config::User>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        /*
        use std::fmt::Write;

        let mut rule_string: String = "has:media -is:retweet ".to_string();

        for (i, user) in users.iter().enumerate() {
            if i < users.len() - 1 {
                write!(
                    &mut rule_string,
                    "(from:{} "{}") OR",
                    user.twitter_id, user.schedule_keyword
                )
                .expect("Writing into buffer failed.");
            } else {
                write!(
                    &mut rule_string,
                    "(from:{} "{}")",
                    user.twitter_id, user.schedule_keyword
                )
                .expect("Writing into buffer failed.");
            }
        }
        */

        let rule_string = "hololive has:media -is:retweet -is:quote".to_string();

        let update: RuleUpdate = RuleUpdate {
            add: vec![Rule {
                value: rule_string,
                tag: Some("Hololive Schedules".to_string()),
            }],
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

    async fn connect(
        client: &Client,
    ) -> Result<impl Stream<Item = Result<Bytes, Error>>, Box<dyn std::error::Error>> {
        let response = client
            .get("https://api.twitter.com/2/tweets/search/stream")
            .query(&[
                ("expansions", "attachments.media_keys"),
                ("media.fields", "url"),
                ("tweet.fields", "author_id"),
            ])
            .send()
            .await
            .unwrap();

        TwitterAPI::check_rate_limit(&response)?;
        response.error_for_status_ref()?;

        Ok(response.bytes_stream())
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

        println!(
            "[TWITTER] {}/{} requests made (Resets {})",
            limit - remaining,
            limit,
            humanized_time.to_text_en(Accuracy::Precise, Tense::Future)
        );

        if remaining <= 0 {
            Err(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                "[TWITTER] Rate limit reached.",
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

#[derive(Serialize)]
struct RuleUpdate {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    add: Vec<Rule>,
    #[serde(skip_serializing_if = "IDList::is_empty")]
    delete: IDList,
}

#[derive(Serialize, Debug)]
struct Rule {
    value: String,
    tag: Option<String>,
}

#[derive(Serialize)]
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
    includes: Expansions,
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

#[derive(Deserialize, Debug)]
struct TweetInfo {
    attachments: TweetAttachments,
    #[serde(with = "super::serializers::string_to_number")]
    author_id: u64,
    #[serde(with = "super::serializers::string_to_number")]
    id: u64,
    text: String,
    #[serde(with = "super::serializers::utc_datetime")]
    created_at: DateTime<Utc>,
}

#[derive(Deserialize, Debug)]
struct TweetAttachments {
    media_keys: Vec<String>,
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
#[derive(Deserialize, Debug)]
struct RemoteRule {
    #[serde(with = "super::serializers::string_to_number")]
    id: u64,
    value: String,
    tag: Option<String>,
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
