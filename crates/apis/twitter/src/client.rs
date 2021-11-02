use std::{error::Error as _, io::ErrorKind, time::Duration};

use backoff::ExponentialBackoff;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use tokio::{
    sync::mpsc::{self, error::TrySendError},
    time::timeout,
};
use tracing::{debug, error, trace, warn};

use crate::{
    errors::{Error, ValidationError},
    types::*,
    util::{check_rate_limit, try_run_with_config, validate_json_bytes, validate_response, VecExt},
};

pub struct FilteredStream {
    channel: mpsc::Receiver<Tweet>,
}

impl FilteredStream {
    pub async fn new<R, It>(
        token: &str,
        rules: It,
        parameters: FilteredStreamParameters,
    ) -> Result<Self, Error>
    where
        R: Into<Rule>,
        It: IntoIterator<Item = R>,
    {
        Self::with_buffer_size(token, rules, parameters, 64).await
    }

    pub async fn with_buffer_size<R, It>(
        token: &str,
        rules: It,
        parameters: FilteredStreamParameters,
        buffer_size: usize,
    ) -> Result<Self, Error>
    where
        R: Into<Rule>,
        It: IntoIterator<Item = R>,
    {
        let rules = rules.into_iter().map(|r| r.into()).collect();
        let channel = FilteredStreamInner::create(token, rules, parameters, buffer_size).await?;

        Ok(Self { channel })
    }
}

impl Stream for FilteredStream {
    type Item = Tweet;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.channel.poll_recv(cx)
    }
}

impl AsRef<mpsc::Receiver<Tweet>> for FilteredStream {
    fn as_ref(&self) -> &mpsc::Receiver<Tweet> {
        &self.channel
    }
}

impl AsMut<mpsc::Receiver<Tweet>> for FilteredStream {
    fn as_mut(&mut self) -> &mut mpsc::Receiver<Tweet> {
        &mut self.channel
    }
}

struct FilteredStreamInner {
    client: reqwest::Client,
}

impl FilteredStreamInner {
    const API_ENDPOINT: &'static str = "https://api.twitter.com";
    const USER_AGENT: &'static str =
        concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

    async fn create(
        token: &str,
        rules: Vec<Rule>,
        parameters: FilteredStreamParameters,
        buffer_size: usize,
    ) -> Result<mpsc::Receiver<Tweet>, Error> {
        use reqwest::header;

        let token = if token.starts_with("Bearer ") {
            token.to_owned()
        } else {
            format!("Bearer {}", token)
        };

        let mut headers = header::HeaderMap::new();

        let mut auth_val =
            header::HeaderValue::from_str(&token).map_err(|_| Error::InvalidApiToken)?;
        auth_val.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, auth_val);

        let client = reqwest::ClientBuilder::new()
            .user_agent(Self::USER_AGENT)
            .default_headers(headers)
            .build()
            .map_err(Error::HttpClientCreationError)?;

        let stream = Self { client };

        stream.setup_rules(rules).await?;
        debug!("Twitter rules set up!");

        let (tx, rx) = mpsc::channel(buffer_size);

        tokio::spawn(async {
            match stream.run(tx, parameters).await {
                Ok(_) => (),
                Err(e) => {
                    error!("{:?}", e);
                }
            }
        });

        Ok(rx)
    }

    async fn setup_rules(&self, rules: Vec<Rule>) -> Result<(), Error> {
        let existing_rules = self.get_rules().await?;

        if rules == existing_rules {
            return Ok(());
        }

        self.delete_rules(existing_rules).await?;

        let update: RuleUpdate = RuleUpdate {
            add: rules,
            delete: IdList { ids: Vec::new() },
        };

        let response = self
            .client
            .post(format!(
                "{}/2/tweets/search/stream/rules",
                Self::API_ENDPOINT
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

        Ok(())
    }

    async fn connect(
        &self,
        parameters: &FilteredStreamParameters,
    ) -> Result<impl Stream<Item = Result<Bytes, reqwest::Error>>, Error> {
        try_run_with_config(
            || async {
                let response = self
                    .client
                    .get("https://api.twitter.com/2/tweets/search/stream")
                    .query(
                        parameters, /* &[
                                       ("expansions", "attachments.media_keys,referenced_tweets.id"),
                                       ("media.fields", "url"),
                                       (
                                           "tweet.fields",
                                           "author_id,created_at,lang,in_reply_to_user_id,referenced_tweets",
                                       ),
                                   ] */
                    )
                    .send()
                    .await
                    .map_err(|e| {
                        warn!("{:?}", e);

                        Error::ApiRequestFailed {
                            endpoint: "GET /2/tweets/search/stream",
                            source: e,
                        }
                    })?;

                check_rate_limit(&response).map_err(|e| {
                    warn!("{:?}", e);
                    e
                })?;

                response.error_for_status_ref().map_err(|e| {
                    warn!("{:?}", e);
                    Error::InvalidResponse {
                        endpoint: "GET /2/tweets/search/stream",
                        source: ValidationError::ServerError(e.into()),
                    }
                })?;

                Ok(response.bytes_stream())
            },
            ExponentialBackoff {
                initial_interval: Duration::from_secs(60),
                max_interval: Duration::from_secs(64 * 60),
                randomization_factor: 0.0,
                multiplier: 2.0,
                ..ExponentialBackoff::default()
            },
        )
        .await
    }

    async fn run(
        self,
        sender: mpsc::Sender<Tweet>,
        parameters: FilteredStreamParameters,
    ) -> Result<(), Error> {
        loop {
            let mut stream = Box::pin(self.connect(&parameters).await?);
            debug!("Connected to Twitter stream!");

            loop {
                let res = timeout(Duration::from_secs(30), stream.next()).await;

                let res = match res {
                    Ok(r) => r,
                    Err(e) => {
                        warn!(error = ?e, "Stream timed out, restarting!");
                        break;
                    }
                };

                let item = match res {
                    Some(m) => m,
                    None => {
                        debug!("Stream disconnected, reconnecting...");
                        break;
                    }
                };

                match item {
                    Ok(message) => {
                        if message == "\r\n" {
                            continue;
                        }

                        let tweet = match self.parse_message(&message).await {
                            Ok(t) => t,
                            Err(e) => {
                                error!("{:?}", e);
                                continue;
                            }
                        };

                        trace!("Tweet successfully parsed!");

                        match sender.try_send(tweet) {
                            Ok(_) => (),
                            Err(TrySendError::Full(_)) => {
                                warn!("Buffer full, dropping tweet!");
                                continue;
                            }
                            Err(TrySendError::Closed(_)) => {
                                debug!("Stream receiver dropped, halting stream.");
                                return Ok(());
                            }
                        }
                    }
                    Err(ref err) => {
                        let hyper_error: Option<&hyper::Error> =
                            err.source().and_then(|e| e.downcast_ref());
                        let io_error: Option<&std::io::Error> = hyper_error
                            .and_then(|e| e.source())
                            .and_then(|e| e.downcast_ref());

                        if let Some(e) = io_error {
                            match e.kind() {
                                ErrorKind::UnexpectedEof => (),
                                _ => {
                                    error!(err = %e, "IO Error, restarting!");
                                    break;
                                }
                            }
                        } else {
                            error!(err = %err, "Error, restarting!");
                            break;
                        }
                    }
                }
            }
        }
    }

    async fn parse_message(&self, message: &Bytes) -> Result<Tweet, Error> {
        trace!("Received twitter message.");

        let response: TweetOrError =
            validate_json_bytes(message).map_err(|e| Error::InvalidResponse {
                endpoint: "GET /2/tweets/search/stream",
                source: e.into(),
            })?;

        let mut tweet = match response {
            TweetOrError::Tweet(t) => t,
            TweetOrError::Error { errors } => {
                error!("Received {} errors!", errors.len());

                for e in &errors {
                    error!("{:?}", e);
                }

                return Err(Error::ApiErrors(errors));
            }
        };

        debug!("New tweet");
        trace!(?tweet, "Tweet parsed.");

        tweet.data.text = tweet.data.text.replace("&amp", "&");

        Ok(tweet)
    }

    async fn get_rules(&self) -> Result<Vec<RemoteRule>, Error> {
        let response = self
            .client
            .get("https://api.twitter.com/2/tweets/search/stream/rules")
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

        Ok(response.data)
    }

    async fn delete_rules(&self, rules: Vec<RemoteRule>) -> Result<(), Error> {
        let request = RuleUpdate {
            add: Vec::new(),
            delete: IdList {
                ids: rules.iter().map(|r| r.id).collect(),
            },
        };

        if rules.is_empty() {
            return Ok(());
        }

        let response = self
            .client
            .post("https://api.twitter.com/2/tweets/search/stream/rules")
            .json(&request)
            .send()
            .await
            .map_err(|e| Error::ApiRequestFailed {
                endpoint: "POST /2/tweets/search/stream/rules",
                source: e,
            })?;

        check_rate_limit(&response)?;

        let _response: RuleUpdateResponse =
            validate_response(response)
                .await
                .map_err(|e| Error::InvalidResponse {
                    endpoint: "POST /2/tweets/search/stream/rules",
                    source: e,
                })?;

        Ok(())
    }
}
