use std::{error::Error as _, io::ErrorKind, time::Duration};

use backoff::ExponentialBackoff;
use futures_lite::{Stream, StreamExt};
use hyper::{body::Bytes, client::HttpConnector, header, Body, Client, Request, Uri};
use tokio::{
    sync::mpsc::{self, error::TrySendError},
    time::{error::Elapsed, timeout},
};
use tracing::{debug, error, trace, warn};

use crate::{
    errors::{Error, ServerError, ValidationError},
    types::*,
    util::{check_rate_limit, try_run_with_config, validate_json_bytes},
};

pub(crate) enum MessageType {
    Tweet(Tweet),
    Timeout(Elapsed),
    Disconnection,
    Error(Error),
    IoError(std::io::ErrorKind),
    NetError(hyper::Error),
    Skip,
}

pub(crate) struct TwitterStream {
    client: Client<HttpConnector>,
    token: String,
    endpoint: &'static str,
}

impl TwitterStream {
    pub const API_ENDPOINT: &'static str = "https://api.twitter.com";
    pub const USER_AGENT: &'static str =
        concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

    pub async fn create(
        endpoint: &'static str,
        token: String,
        client: Client<HttpConnector>,
        parameters: StreamParameters,
        buffer_size: usize,
    ) -> Result<(mpsc::Receiver<Tweet>, mpsc::Sender<()>), Error> {
        let mut stream = Self {
            client,
            token,
            endpoint,
        };

        let (tx, rx) = mpsc::channel(buffer_size);
        let (exit_tx, exit_rx) = mpsc::channel(1);

        tokio::spawn(async move {
            match stream.run(tx, exit_rx, parameters).await {
                Ok(_) => (),
                Err(e) => {
                    error!("{:?}", e);
                }
            }
        });

        Ok((rx, exit_tx))
    }

    async fn connect(
        &self,
        parameters: &StreamParameters,
    ) -> Result<impl Stream<Item = Result<Bytes, hyper::Error>>, Error> {
        let query = serde_urlencoded::to_string(parameters).unwrap();

        try_run_with_config(
            || async {
                let request = Request::get(
                    format!("{}{}?{}", Self::API_ENDPOINT, self.endpoint, query)
                        .parse::<Uri>()
                        .unwrap(),
                )
                .header(header::USER_AGENT, Self::USER_AGENT)
                .header(header::AUTHORIZATION, &self.token)
                .body(Body::empty())
                .unwrap();

                let response = self.client.request(request).await.map_err(|e| {
                    warn!("{:?}", e);

                    Error::ApiRequestFailed {
                        endpoint: self.endpoint,
                        source: e,
                    }
                })?;

                check_rate_limit(&response).map_err(|e| {
                    warn!("{:?}", e);
                    e
                })?;

                let status = response.status();

                if status.is_client_error() || status.is_server_error() {
                    warn!("{:?}", status);

                    return Err(Error::InvalidResponse {
                        endpoint: self.endpoint,
                        source: ValidationError::ServerError(ServerError::ErrorCode(status)),
                    });
                }

                Ok(response.into_body())
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
        &mut self,
        sender: mpsc::Sender<Tweet>,
        mut exit_receiver: mpsc::Receiver<()>,
        parameters: StreamParameters,
    ) -> Result<(), Error> {
        loop {
            let mut stream = Box::pin(self.connect(&parameters).await?);
            debug!("Connected to Twitter stream!");

            loop {
                tokio::select! {
                    res = timeout(Duration::from_secs(30), stream.next()) => {
                        let tweet = match self.handle_possible_message(res).await {
                            MessageType::Tweet(t) => {
                                trace!("Tweet successfully parsed!");
                                t
                            }
                            MessageType::Skip => {
                                continue;
                            }
                            MessageType::Error(e) => {
                                error!("{:?}", e);
                                continue;
                            }
                            MessageType::Timeout(e) => {
                                warn!(error = ?e, "Stream timed out, restarting!");
                                break;
                            }
                            MessageType::Disconnection => {
                                debug!("Stream disconnected, reconnecting...");
                                break;
                            }
                            MessageType::NetError(e) => {
                                error!(error = ?e, "Error, restarting!");
                                break;
                            }
                            MessageType::IoError(e) => {
                                error!(error = ?e, "IO Error, restarting!");
                                break;
                            }
                        };

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

                    _ = exit_receiver.recv() => {
                        return Ok(());
                    }
                }
            }
        }
    }

    async fn handle_possible_message(
        &self,
        message: Result<Option<Result<Bytes, hyper::Error>>, Elapsed>,
    ) -> MessageType {
        match message {
            Ok(Some(Ok(msg))) if msg == "\r\n" => MessageType::Skip,
            Ok(Some(Ok(msg))) => match self.parse_message(&msg).await {
                Ok(t) => MessageType::Tweet(t),
                Err(e) => MessageType::Error(e),
            },
            Ok(Some(Err(err))) => {
                let hyper_error: Option<&hyper::Error> =
                    err.source().and_then(|e| e.downcast_ref());
                let io_error: Option<&std::io::Error> = hyper_error
                    .and_then(|e| e.source())
                    .and_then(|e| e.downcast_ref());

                if let Some(e) = io_error {
                    match e.kind() {
                        ErrorKind::UnexpectedEof => MessageType::Skip,
                        _ => MessageType::IoError(e.kind()),
                    }
                } else {
                    MessageType::NetError(err)
                }
            }
            Ok(None) => MessageType::Disconnection,
            Err(e) => MessageType::Timeout(e),
        }
    }

    async fn parse_message(&self, message: &[u8]) -> Result<Tweet, Error> {
        trace!("Received twitter message.");

        let response: TweetOrError =
            validate_json_bytes(message).map_err(|e| Error::InvalidResponse {
                endpoint: self.endpoint,
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

        Self::correct_encoding_errors(&mut tweet);

        Ok(tweet)
    }

    fn correct_encoding_errors(tweet: &mut Tweet) {
        tweet.data.text = tweet.data.text.replace("&amp", "&");
    }
}
