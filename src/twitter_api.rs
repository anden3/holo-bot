use bytes::Bytes;
use chrono::prelude::*;
use futures::Stream;
use reqwest::{Error, Response};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub struct TwitterAPI {
    client: reqwest::Client,
}

#[derive(Deserialize)]
pub struct User {
    name: String,
    handle: String,
    twitter_id: u64,
    keyword: String,
}

#[derive(Serialize)]
struct Rule {
    value: String,
    tag: Option<String>,
}

#[derive(Deserialize)]
struct RemoteRule {
    id: String,
    value: String,
    tag: String,
}

#[derive(Serialize)]
struct IDList {
    ids: Vec<u64>,
}

#[derive(Deserialize)]
struct RuleUpdateResponseMetaSummary {
    created: i32,
    not_created: i32,
}

#[derive(Deserialize)]
struct RuleUpdateResponseMeta {
    remaining: i64,
    summary: RuleUpdateResponseMetaSummary,
}

#[derive(Serialize)]
struct RuleUpdate {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    add: Vec<Rule>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    delete: Vec<IDList>,
}

#[derive(Deserialize)]
struct RuleUpdateResponse {
    data: Vec<RemoteRule>,
    meta: RuleUpdateResponseMeta,
}

impl TwitterAPI {
    pub fn new(bearer_token: &str) -> TwitterAPI {
        use reqwest::header;

        let formatted_token = format!("Bearer {}", bearer_token);
        let mut headers = header::HeaderMap::new();

        let mut auth_val = header::HeaderValue::from_str(&formatted_token).unwrap();
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
            .expect("Failed to build client.");

        return TwitterAPI { client };
    }

    pub async fn setup_rules(&self, users: &Vec<User>) -> Result<(), Box<dyn std::error::Error>> {
        use std::fmt::Write;

        /*
        let mut rule_string: String = "has:media -is:retweet ".to_string();

        for (i, user) in users.iter().enumerate() {
            if i < users.len() - 1 {
                write!(
                    &mut rule_string,
                    "(from:{} {}) OR",
                    user.twitter_id, user.keyword
                )
                .expect("Writing into buffer failed.");
            } else {
                write!(
                    &mut rule_string,
                    "(from:{} {})",
                    user.twitter_id, user.keyword
                )
                .expect("Writing into buffer failed.");
            }
        }
        */

        let rule_string = "hololive".to_string();

        let update: RuleUpdate = RuleUpdate {
            add: vec![Rule {
                value: rule_string,
                tag: Some("Hololive Schedules".to_string()),
            }],
            delete: vec![],
        };

        let response = self
            .client
            .post("https://api.twitter.com/2/tweets/search/stream/rules")
            .json(&update)
            .send()
            .await
            .expect("Rule update failed.");

        TwitterAPI::check_rate_limit(&response)?;
        TwitterAPI::check_response_for_errors::<RuleUpdateResponse>(response).await?;

        Ok(())
    }

    pub async fn connect(
        &self,
    ) -> Result<impl Stream<Item = Result<Bytes, Error>>, Box<dyn std::error::Error>> {
        let response = self
            .client
            .get("https://api.twitter.com/2/tweets/search/stream")
            .query(&[
                ("expansions", "attachments.media_keys,author_id"),
                ("media.fields", "media_key,url"),
                ("tweet.fields", "attachments"),
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

    async fn check_response_for_errors<T>(response: Response) -> Result<(), Error>
    where
        T: DeserializeOwned,
    {
        if let Err(error_code) = (&response).error_for_status_ref() {
            let response: serde_json::Value = serde_json::from_str(&response.text().await.unwrap())
                .expect("Deserialization of Error failed.");

            println!("Error: {:#?}", response);

            return Err(error_code);
        }

        Ok(())
    }
}
