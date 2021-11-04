use std::collections::HashMap;

use futures::Stream;
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
    client: reqwest::Client,
    tweet_stream: mpsc::Receiver<Tweet>,
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
        let client = TwitterStream::initialize_client(token)?;

        let (tweet_stream, exit_notifier) = TwitterStream::create(
            "/2/tweets/search/stream",
            client.clone(),
            parameters,
            buffer_size,
        )
        .await?;

        let mut stream = Self {
            client,
            tweet_stream,
            exit_notifier,
            rules: HashMap::new(),
        };

        stream.rules = stream.fetch_rules().await?;
        debug!("Twitter rules set up!");

        Ok(stream)
    }

    async fn fetch_rules(&self) -> Result<HashMap<RuleId, ActiveRule>, Error> {
        let response = self
            .client
            .get(format!(
                "{}/2/tweets/search/stream/rules",
                TwitterStream::API_ENDPOINT
            ))
            .send()
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

        let response = self
            .client
            .post(format!(
                "{}/2/tweets/search/stream/rules",
                TwitterStream::API_ENDPOINT
            ))
            .json(&update)
            .send()
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
        let request = RuleUpdate::remove(rules.to_vec());

        let response = self
            .client
            .post(format!(
                "{}/2/tweets/search/stream/rules",
                TwitterStream::API_ENDPOINT
            ))
            .json(&request)
            .send()
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
                    rules_to_be_deleted: request.delete.ids,
                });
            }
        }

        self.rules.retain(|id, _| !request.delete.ids.contains(id));

        Ok(())
    }

    pub async fn validate_rules(&self, rules: &[Rule]) -> Result<(), Error> {
        let update = RuleUpdate::add(rules.to_vec());

        let response = self
            .client
            .post(format!(
                "{}/2/tweets/search/stream/rules",
                TwitterStream::API_ENDPOINT
            ))
            .query(&[("dry_run", true)])
            .json(&update)
            .send()
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
