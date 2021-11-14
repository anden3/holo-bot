use std::collections::HashMap;

use futures_lite::Stream;
use hyper::{client::HttpConnector, header, Body, Client, Request};
use tokio::sync::mpsc::{self};
use tracing::{debug, error};

use crate::{
    errors::Error,
    streams::twitter_stream::TwitterStream,
    types::id::*,
    types::*,
    util::{check_rate_limit, validate_response, VecExt},
};

pub struct FilteredStream {
    client: hyper::client::Client<HttpConnector>,
    tweet_stream: mpsc::Receiver<Tweet>,
    token: String,
    rules: HashMap<RuleId, ActiveRule>,
    exit_notifier: mpsc::Sender<()>,
}

impl FilteredStream {
    pub async fn new(token: &str, parameters: StreamParameters) -> Result<Self, Error> {
        Self::with_buffer_size(token, parameters, 64).await
    }

    pub async fn with_buffer_size(
        token: &str,
        parameters: StreamParameters,
        buffer_size: usize,
    ) -> Result<Self, Error> {
        let client = Client::new();

        let token = if token.starts_with("Bearer ") {
            token.to_owned()
        } else {
            format!("Bearer {}", token)
        };

        let (tweet_stream, exit_notifier) = TwitterStream::create(
            "/2/tweets/search/stream",
            token.clone(),
            client.clone(),
            parameters,
            buffer_size,
        )
        .await?;

        let mut stream = Self {
            client,
            tweet_stream,
            token,
            exit_notifier,
            rules: HashMap::new(),
        };

        stream.rules = stream.fetch_rules().await?;
        debug!("Twitter rules set up!");

        Ok(stream)
    }

    async fn fetch_rules(&self) -> Result<HashMap<RuleId, ActiveRule>, Error> {
        let request = Request::get(
            format!(
                "{}/2/tweets/search/stream/rules",
                TwitterStream::API_ENDPOINT
            )
            .parse::<hyper::Uri>()
            .unwrap(),
        )
        .header(header::USER_AGENT, TwitterStream::USER_AGENT)
        .header(header::AUTHORIZATION, &self.token)
        .body(Body::empty())
        .unwrap();

        let response = self
            .client
            .request(request)
            .await
            .map_err(|e| Error::ApiRequestFailed {
                endpoint: "GET /2/tweets/search/stream/rules",
                source: e,
            })?;

        check_rate_limit(&response)?;

        let mut response: RuleRequestResponse =
            validate_response(response)
                .await
                .map_err(|e| Error::InvalidResponse {
                    endpoint: "GET /2/tweets/search/stream/rules",
                    source: e,
                })?;

        response.data.sort_unstable_by_key_ref(|r| &r.tag);
        let rules = response.data.into_iter().map(|r| (r.id, r)).collect();

        Ok(rules)
    }

    pub async fn set_rules(&mut self, rules: Vec<Rule>) -> Result<(), Error> {
        if rules.iter().eq(self.rules.values()) {
            return Ok(());
        }

        self.remove_rules(&self.rules.keys().copied().collect::<Vec<_>>())
            .await?;

        self.add_rules(&rules).await?;

        Ok(())
    }

    pub async fn add_rules(&mut self, rules: &[Rule]) -> Result<(), Error> {
        let update = RuleUpdate::add(rules.to_vec());

        let request = Request::post(
            format!(
                "{}/2/tweets/search/stream/rules",
                TwitterStream::API_ENDPOINT
            )
            .parse::<hyper::Uri>()
            .unwrap(),
        )
        .header(header::USER_AGENT, TwitterStream::USER_AGENT)
        .header(header::AUTHORIZATION, &self.token)
        .body(serde_json::to_vec(&update).unwrap().into())
        .unwrap();

        let response = self
            .client
            .request(request)
            .await
            .map_err(|e| Error::ApiRequestFailed {
                endpoint: "POST /2/tweets/search/stream/rules",
                source: e,
            })?;

        check_rate_limit(&response)?;

        let response: RuleUpdateResponse =
            validate_response(response)
                .await
                .map_err(|e| Error::InvalidResponse {
                    endpoint: "POST /2/tweets/search/stream/rules",
                    source: e,
                })?;

        if let Some(meta) = response.meta {
            if meta.summary.invalid > 0 {
                error!(count = meta.summary.invalid, rules = ?update.add, "Invalid rules found!");

                return Err(Error::InvalidRules {
                    count: meta.summary.invalid,
                    rules: update.add,
                });
            }
        }

        let new_rules = response
            .data
            .unwrap_or_default()
            .into_iter()
            .map(|r| (r.id, r));

        self.rules.extend(new_rules);
        Ok(())
    }

    pub async fn remove_rules(&mut self, rules: &[RuleId]) -> Result<(), Error> {
        if rules.is_empty() {
            return Ok(());
        }

        let rule_count = rules.len();
        let update = RuleUpdate::remove(rules.to_vec());

        let request = Request::post(
            format!(
                "{}/2/tweets/search/stream/rules",
                TwitterStream::API_ENDPOINT
            )
            .parse::<hyper::Uri>()
            .unwrap(),
        )
        .header(header::USER_AGENT, TwitterStream::USER_AGENT)
        .header(header::AUTHORIZATION, &self.token)
        .body(serde_json::to_vec(&update).unwrap().into())
        .unwrap();

        let response = self
            .client
            .request(request)
            .await
            .map_err(|e| Error::ApiRequestFailed {
                endpoint: "POST /2/tweets/search/stream/rules",
                source: e,
            })?;

        check_rate_limit(&response)?;

        let response: RuleUpdateResponse =
            validate_response(response)
                .await
                .map_err(|e| Error::InvalidResponse {
                    endpoint: "POST /2/tweets/search/stream/rules",
                    source: e,
                })?;

        if let Some(meta) = response.meta {
            if meta.summary.deleted != rule_count || meta.summary.not_deleted > 0 {
                error!(
                    count = meta.summary.deleted,
                    expected = rule_count,
                    "Deleted rules count mismatch!"
                );

                let not_deleted =
                    std::cmp::max(rule_count - meta.summary.deleted, meta.summary.not_deleted);

                return Err(Error::RuleDeletionFailed {
                    failed_deletion_count: not_deleted,
                    rules_to_be_deleted: update.delete.ids,
                });
            }
        }

        self.rules.retain(|id, _| !update.delete.ids.contains(id));

        Ok(())
    }

    pub async fn validate_rules(&self, rules: &[Rule]) -> Result<(), Error> {
        let update = RuleUpdate::add(rules.to_vec());

        let request = Request::post(
            format!(
                "{}/2/tweets/search/stream/rules?dry_run=true",
                TwitterStream::API_ENDPOINT
            )
            .parse::<hyper::Uri>()
            .unwrap(),
        )
        .header(header::USER_AGENT, TwitterStream::USER_AGENT)
        .header(header::AUTHORIZATION, &self.token)
        .body(serde_json::to_vec(&update).unwrap().into())
        .unwrap();

        let response = self
            .client
            .request(request)
            .await
            .map_err(|e| Error::ApiRequestFailed {
                endpoint: "POST /2/tweets/search/stream/rules",
                source: e,
            })?;

        check_rate_limit(&response)?;

        let response: RuleUpdateResponse =
            validate_response(response)
                .await
                .map_err(|e| Error::InvalidResponse {
                    endpoint: "POST /2/tweets/search/stream/rules",
                    source: e,
                })?;

        if let Some(meta) = response.meta {
            if meta.summary.invalid > 0 {
                error!(count = meta.summary.invalid, rules = ?update.add, "Invalid rules found!");

                return Err(Error::InvalidRules {
                    count: meta.summary.invalid,
                    rules: update.add,
                });
            }
        }

        Ok(())
    }
}

impl Stream for FilteredStream {
    type Item = Tweet;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.tweet_stream.poll_recv(cx)
    }
}

impl AsRef<mpsc::Receiver<Tweet>> for FilteredStream {
    fn as_ref(&self) -> &mpsc::Receiver<Tweet> {
        &self.tweet_stream
    }
}

impl AsMut<mpsc::Receiver<Tweet>> for FilteredStream {
    fn as_mut(&mut self) -> &mut mpsc::Receiver<Tweet> {
        &mut self.tweet_stream
    }
}

impl Drop for FilteredStream {
    fn drop(&mut self) {
        let _ = self.exit_notifier.send(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indonesian_language_tag() {
        let value = serde_json::json! {{
            "data": {
                "attachments": {},
                "author_id": "1409817941705515015",
                "created_at": "2021-11-09T20:40:54.000Z",
                "id": "1458172771054206977",
                "lang": "in",
                "referenced_tweets": [
                    {
                        "id": "1458038855005782017",
                        "type": "quoted"
                    }
                ],
                "text": "Ki Ki Kiaraoke!!! ❤\u{fe0f} https://t.co/Petk0vGL7A"
            },
            "includes": {
                "tweets": [
                    {
                        "attachments": {
                            "media_keys": [
                                "3_1458038450557423623"
                            ]
                        },
                        "author_id": "1283646922406760448",
                        "created_at": "2021-11-09T11:48:46.000Z",
                        "id": "1458038855005782017",
                        "lang": "ja",
                        "text": "next stream→【KIARAOKE ENDURANCE】\n\nThe next milestone is close!\n1.25 million, can we do it?!\nI will try to sing as long as possible to reach it!!!!\n125万人が近い...いけるのかな!?耐久カラオケだぁー!\n\np1 unarchived: https://t.co/1NtZyl2rjf\np2 archived: https://t.co/MQnvK22wko https://t.co/vG5MluBG04"
                    }
                ]
            },
            "matching_rules": [
                {
                    "id": "1458025356401778690",
                    "tag": "Hololive Talents #4"
                }
            ]
        }};

        let _: Tweet = serde_json::from_value(value).unwrap();
    }
}
