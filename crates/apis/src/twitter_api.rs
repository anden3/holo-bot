use std::{error::Error as StdError, io::ErrorKind, time::Duration};

use anyhow::{anyhow, Context};
use backoff::ExponentialBackoff;
use bytes::Bytes;
use chrono::prelude::*;
use futures::{Stream, StreamExt};
use reqwest::{Client, Error, Response};
use serde::de::DeserializeOwned;
use tokio::{
    sync::{
        mpsc::{self, Sender, UnboundedReceiver, UnboundedSender},
        watch,
    },
    time::timeout,
};
use tracing::{debug, debug_span, error, info, instrument, trace, warn, Instrument};

use crate::{
    discord_api::DiscordMessageData, translation_api::TranslationApi, types::twitter_api::*,
};
use utility::{
    config::{self, Config},
    extensions::VecExt,
    functions::try_run_with_config,
    here,
};

pub struct TwitterApi;

impl TwitterApi {
    #[instrument(skip(config, notifier_sender, exit_receiver))]
    pub async fn start(
        config: Config,
        notifier_sender: Sender<DiscordMessageData>,
        exit_receiver: watch::Receiver<bool>,
    ) {
        if config.development {
            return;
        }

        let (msg_tx, msg_rx) = mpsc::unbounded_channel::<Bytes>();
        let config_clone = config.clone();
        let exit_rx_clone = exit_receiver.clone();

        tokio::spawn(
            async move {
                match Self::run(config, msg_tx, exit_receiver).await {
                    Ok(_) => (),
                    Err(e) => {
                        error!("{:?}", e);
                    }
                }
            }
            .instrument(debug_span!("Twitter API")),
        );

        tokio::spawn(
            async move {
                match Self::message_consumer(config_clone, msg_rx, notifier_sender, exit_rx_clone)
                    .await
                {
                    Ok(_) => (),
                    Err(e) => {
                        error!("{:?}", e);
                    }
                }
            }
            .instrument(debug_span!("Twitter message consumer")),
        );
    }

    #[instrument(skip(config, message_sender, exit_receiver))]
    async fn run(
        config: Config,
        message_sender: UnboundedSender<Bytes>,
        mut exit_receiver: watch::Receiver<bool>,
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
        debug!("Twitter rules set up!");

        'main: loop {
            let mut stream = Box::pin(Self::connect(&client).await?);
            debug!("Connected to Twitter stream!");

            loop {
                tokio::select! {
                    res = timeout(Duration::from_secs(30), stream.next()) => {
                        let res = match res {
                            Ok(r) => r,
                            Err(e) => {
                                warn!(error = ?e, "Stream timed out, restarting!");
                                break;
                            },
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

                                trace!("Message sent!");
                                message_sender.send(message)?;
                            },
                            Err(ref err) => {
                                let hyper_error: Option<&hyper::Error> = err.source().and_then(|e| e.downcast_ref());
                                let io_error: Option<&std::io::Error> = hyper_error.and_then(|e| e.source()).and_then(|e| e.downcast_ref());

                                if let Some(e) = io_error {
                                    match e.kind() {
                                        ErrorKind::UnexpectedEof => (),
                                        _ => {
                                            error!(err = %e, "IO Error, restarting!");
                                            break;
                                        }
                                    }
                                }
                                else {
                                    error!(err = %err, "Error, restarting!");
                                    break;
                                }
                            }
                        }
                    }

                    res = exit_receiver.changed() => {
                        if let Err(e) = res {
                            error!("{:?}", e);
                        }
                        break 'main;
                    }
                }
            }
        }

        info!(task = "Twitter API", "Shutting down.");
        Ok(())
    }

    #[instrument(skip(config, message_receiver, notifier_sender, exit_receiver))]
    async fn message_consumer(
        config: Config,
        mut message_receiver: UnboundedReceiver<Bytes>,
        notifier_sender: Sender<DiscordMessageData>,
        mut exit_receiver: watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let translator = match TranslationApi::new(&config) {
            Ok(api) => api,
            Err(e) => {
                anyhow::bail!(e);
            }
        };

        loop {
            tokio::select! {
                Some(msg) = message_receiver.recv() => {
                    trace!("Message received from producer!");
                    match Self::parse_message(&msg, &config.users, &translator).await {
                        Ok(Some(discord_message)) => {
                            trace!("Tweet successfully parsed!");
                            notifier_sender
                                .send(discord_message)
                                .await
                                .context(here!())?;
                        }
                        Ok(None) => (),
                        Err(e) => error!("{:?}", e),
                    }
                }

                res = exit_receiver.changed() => {
                    if let Err(e) = res {
                        error!("{:?}", e);
                    }
                    break;
                }
            }
        }

        Ok(())
    }

    #[instrument(skip(client))]
    async fn connect(client: &Client) -> anyhow::Result<impl Stream<Item = Result<Bytes, Error>>> {
        try_run_with_config(
            || async {
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

    #[allow(clippy::too_many_lines)]
    #[instrument(skip(message, users, translator))]
    async fn parse_message(
        message: &Bytes,
        users: &[config::User],
        translator: &TranslationApi,
    ) -> anyhow::Result<Option<DiscordMessageData>> {
        let deserializer = &mut serde_json::Deserializer::from_slice(message);
        let response: Result<Tweet, _> = serde_path_to_error::deserialize(deserializer);

        let mut message = match response {
            Ok(response) => response,
            Err(e) => {
                error!(
                    "Deserialization error at '{}' in {}.",
                    e.path().to_string(),
                    here!()
                );
                error!(
                    "Data:\r\n{:?}",
                    std::str::from_utf8(message).context(here!())?
                );
                return Err(e.into());
            }
        };

        message.data.text = message.data.text.replace("&amp", "&");

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
                    match translator
                        .get_translator_for_lang(&lang)
                        .translate(&message.data.text, &lang)
                        .await
                    {
                        Ok(tl) => {
                            translation = Some(tl);
                        }
                        Err(e) => {
                            error!("{:?}", e);
                        }
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

    #[instrument(skip(client, users))]
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

    #[instrument(skip(client))]
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

    #[instrument(skip(client))]
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

    #[instrument(skip(response))]
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
