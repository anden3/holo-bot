use std::{error::Error as StdError, io::ErrorKind, sync::Arc, time::Duration};

use anyhow::{anyhow, Context};
use backoff::ExponentialBackoff;
use bytes::Bytes;
use chrono::prelude::*;
use futures::{Stream, StreamExt};
use holo_bot_macros::clone_variables;
use reqwest::{Client, Error, Response};
use serde::de::DeserializeOwned;
use tokio::{
    sync::{
        broadcast,
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
    config::{self, Config, ConfigUpdate, Talent, TwitterConfig},
    extensions::VecExt,
    functions::try_run_with_config,
    here,
};

pub struct TwitterApi;

impl TwitterApi {
    #[instrument(skip(config, notifier_sender, exit_receiver))]
    pub async fn start(
        config: Arc<Config>,
        notifier_sender: Sender<DiscordMessageData>,
        exit_receiver: watch::Receiver<bool>,
    ) {
        let (msg_tx, msg_rx) = mpsc::unbounded_channel::<Bytes>();

        tokio::spawn(
            clone_variables!(config, mut exit_receiver; {
                match Self::run(config.twitter.clone(), &config.talents, msg_tx, config.updates.as_ref().unwrap().subscribe(), exit_receiver).await {
                    Ok(_) => (),
                    Err(e) => {
                        error!("{:?}", e);
                    }
                }
            })
            .instrument(debug_span!("Twitter API")),
        );

        tokio::spawn(
            clone_variables!(config, mut exit_receiver; {
                match Self::message_consumer(config.twitter.clone(), &config.talents, msg_rx, notifier_sender, config.updates.as_ref().unwrap().subscribe(), exit_receiver).await {
                    Ok(_) => (),
                    Err(e) => {
                        error!("{:?}", e);
                    }
                }
            })
            .instrument(debug_span!("Twitter message consumer")),
        );
    }

    #[instrument(skip(config, talents))]
    async fn initialize_client(
        config: &TwitterConfig,
        talents: &[Talent],
    ) -> anyhow::Result<Client> {
        use reqwest::header;

        let formatted_token = format!("Bearer {}", &config.token);
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

        let talents_with_twitter = talents
            .iter()
            .filter(|t| t.twitter_id.is_some())
            .collect::<Vec<_>>();

        Self::setup_rules(&client, &talents_with_twitter).await?;
        debug!("Twitter rules set up!");

        Ok(client)
    }

    #[instrument(skip(config, talents, message_sender, config_updates, exit_receiver))]
    async fn run(
        mut config: TwitterConfig,
        talents: &[Talent],
        message_sender: UnboundedSender<Bytes>,
        mut config_updates: broadcast::Receiver<ConfigUpdate>,
        mut exit_receiver: watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let mut client = Self::initialize_client(&config, talents).await?;

        'main: loop {
            if !config.enabled {
                config = Self::wait_to_be_enabled(&mut config_updates).await?;
                client = Self::initialize_client(&config, talents).await?;
            }

            let mut stream = Box::pin(Self::connect(&client).await?);
            debug!("Connected to Twitter stream!");

            loop {
                tokio::select! {
                    res = timeout(Duration::from_secs(30), stream.next()), if config.enabled => {
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

                    update = config_updates.recv() => {
                        use ConfigUpdate::*;

                        let update = match update {
                            Ok(u) => u,
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                warn!(count = n, "Config updates lagged!");
                                continue;
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                return Ok(());
                            }
                        };

                        match update {
                            TwitterEnabled(new_cfg) => {
                                warn!("Received enabled message while already enabled!");
                                assert_eq!(config, new_cfg);
                                continue;
                            }
                            TwitterDisabled => {
                                config.enabled = false;
                                break;
                            }
                            TwitterTokenChanged(new_token) => {
                                config.token = new_token;
                                client = Self::initialize_client(&config, talents).await?;
                                break;
                            }

                            _ => (),
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

    #[instrument(skip(
        config,
        talents,
        message_receiver,
        notifier_sender,
        config_updates,
        exit_receiver
    ))]
    async fn message_consumer(
        mut config: TwitterConfig,
        talents: &[Talent],
        mut message_receiver: UnboundedReceiver<Bytes>,
        notifier_sender: Sender<DiscordMessageData>,
        mut config_updates: broadcast::Receiver<ConfigUpdate>,
        mut exit_receiver: watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let mut translator = TranslationApi::new(&config.feed_translation)?;

        let talents_with_twitter = talents
            .iter()
            .filter(|t| t.twitter_id.is_some())
            .collect::<Vec<_>>();

        loop {
            tokio::select! {
                Some(msg) = message_receiver.recv() => {
                    trace!("Message received from producer!");
                    match Self::parse_message(&msg, &talents_with_twitter, &translator).await {
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

                update = config_updates.recv() => {
                    use ConfigUpdate::*;

                    let update = match update {
                        Ok(u) => u,
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!(count = n, "Config updates lagged!");
                            continue;
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            return Ok(());
                        }
                    };

                    match update {
                        TranslatorAdded(tl_type, tl_config) => {
                            config.feed_translation.insert(tl_type, tl_config);
                            translator = TranslationApi::new(&config.feed_translation)?;
                        }

                        TranslatorRemoved(tl_type) => {
                            config.feed_translation.remove(&tl_type);
                            translator = TranslationApi::new(&config.feed_translation)?;
                        }

                        TranslatorChanged(tl_type, tl_config) => {
                            if let Some(old_config) = config.feed_translation.get_mut(&tl_type) {
                                *old_config = tl_config;
                                translator = TranslationApi::new(&config.feed_translation)?;
                            }
                        }

                        _ => (),
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

    #[instrument(skip(updates))]
    async fn wait_to_be_enabled(
        updates: &mut broadcast::Receiver<ConfigUpdate>,
    ) -> anyhow::Result<TwitterConfig> {
        loop {
            match updates.recv().await {
                Ok(ConfigUpdate::TwitterEnabled(new_config)) => {
                    return Ok(new_config);
                }

                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(count = n, "Config updates lagged!");
                }
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(anyhow::anyhow!("Config updates closed!"));
                }

                _ => (),
            }
        }
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

    #[instrument(skip(message, talents, translator))]
    async fn parse_message(
        message: &Bytes,
        talents: &[&Talent],
        translator: &TranslationApi,
    ) -> anyhow::Result<Option<DiscordMessageData>> {
        trace!("Received twitter message.");

        let deserializer = &mut serde_json::Deserializer::from_slice(message);
        let response: Result<TweetOrError, _> = serde_path_to_error::deserialize(deserializer);

        let mut tweet = match response {
            Ok(TweetOrError::Tweet(t)) => t,
            Ok(TweetOrError::Error { errors }) => {
                error!("Received {} errors!", errors.len());

                for e in errors {
                    error!("{:?}", e);
                }

                return Ok(None);
            }
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

        trace!(?tweet, "Tweet parsed.");

        tweet.data.text = tweet.data.text.replace("&amp", "&");

        // Find who made the tweet.
        let talent = talents
            .iter()
            .find(|u| u.twitter_id.unwrap() == tweet.data.author_id)
            .ok_or({
                anyhow!(
                    "Could not find user with twitter ID: {}",
                    tweet.data.author_id
                )
            })
            .context(here!())?;

        trace!(talent = %talent.english_name, "Found talent who sent tweet.");

        // Check for schedule keyword.
        if let Some(schedule_update) = tweet.schedule_update(talent) {
            info!("New schedule update from {}.", talent.english_name);
            return Ok(Some(DiscordMessageData::ScheduleUpdate(schedule_update)));
        }

        // Check if we're replying to another talent.
        let replied_to = tweet.talent_reply(talents);

        // Add attachments if they exist.
        let media = tweet.attached_photos().map(|p| p.to_owned()).collect();

        // Check if translation is necessary.
        let translation = tweet.translate(translator).await;

        info!("New tweet from {}.", talent.english_name);

        Ok(Some(DiscordMessageData::Tweet(HoloTweet {
            id: tweet.data.id,
            user: <config::Talent as Clone>::clone(talent),
            text: tweet.data.text,
            link: format!(
                "https://twitter.com/{}/status/{}",
                talent.twitter_handle.as_ref().unwrap(),
                tweet.data.id
            ),
            timestamp: tweet.data.created_at,
            media,
            translation,
            replied_to,
        })))
    }

    #[instrument(skip(client, talents))]
    async fn setup_rules(client: &Client, talents: &[&config::Talent]) -> anyhow::Result<()> {
        const RULE_PREFIX: &str = "-is:retweet (";
        const RULE_SUFFIX: &str = ")";
        const RULE_SEPARATOR: &str = " OR ";
        const ID_PREFIX: &str = "from:";

        const RULE_MAX_LEN: usize = 512;
        const ID_MAX_LEN: usize = 19;
        const ID_WITH_PREFIX_LEN: usize = ID_MAX_LEN + ID_PREFIX.len();
        const RULE_MAX_LEN_WITHOUT_FIXES: usize =
            RULE_MAX_LEN - RULE_PREFIX.len() - RULE_SUFFIX.len();

        const MAX_IDS_PER_RULE: usize = (RULE_MAX_LEN_WITHOUT_FIXES + RULE_SEPARATOR.len())
            / (ID_WITH_PREFIX_LEN + RULE_SEPARATOR.len());

        debug_assert_eq!(ID_MAX_LEN, u64::MAX.to_string().len());

        let rules = talents
            .iter()
            .map(|t| format!("{}{}", ID_PREFIX, t.twitter_id.unwrap()))
            .collect::<Vec<_>>()
            .chunks(MAX_IDS_PER_RULE)
            .enumerate()
            .map(|(i, chunk)| Rule {
                value: format!(
                    "{}{}{}",
                    RULE_PREFIX,
                    chunk.join(RULE_SEPARATOR),
                    RULE_SUFFIX
                ),
                tag: format!("Hololive Talents #{}", i + 1),
            })
            .collect::<Vec<_>>();

        trace!(?rules, "Rules");

        let existing_rules = Self::get_rules(client).await?;

        if rules == existing_rules {
            return Ok(());
        }

        warn!("Filter rule mismatch! Was the list of talents recently changed perhaps?");
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
        let response: RuleUpdateResponse = Self::validate_response(response).await?;

        if let Some(meta) = response.meta {
            if meta.summary.invalid > 0 {
                error!(count = meta.summary.invalid, rules = ?update.add, "Invalid rules found!");

                return Err(anyhow!(
                    "{} invalid rules found! Rules are {:#?}.",
                    meta.summary.invalid,
                    update.add
                ));
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

    #[instrument(skip(client, rules))]
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
        let reset: DateTime<Utc> = DateTime::from_utc(reset, Utc);

        // Get duration until reset happens.
        let humanized_time = chrono_humanize::HumanTime::from(Utc::now() - reset);

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
    pub user: config::Talent,
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
