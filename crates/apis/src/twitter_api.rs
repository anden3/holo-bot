use std::sync::Arc;

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use chrono::prelude::*;
use futures::StreamExt;
use itertools::Itertools;
use tokio::sync::{broadcast, mpsc::Sender, watch};
use tracing::{debug_span, error, info, instrument, trace, warn, Instrument};
use twitter::{
    FilteredStream, FilteredStreamParameters, MediaField, RequestedExpansion, Rule, Tweet,
    TweetField,
};

use crate::{discord_api::DiscordMessageData, translation_api::TranslationApi};
use utility::{
    config::{self, Config, ConfigUpdate, Talent, TwitterConfig},
    here,
};

#[async_trait]
trait TweetExt {
    async fn translate(&self, translator: &TranslationApi) -> Option<String>;
    fn schedule_update(&self, talent: &Talent) -> Option<ScheduleUpdate>;
    fn talent_reply(&self, talents: &[Talent]) -> Option<HoloTweetReference>;
}

#[async_trait]
impl TweetExt for Tweet {
    async fn translate(&self, translator: &TranslationApi) -> Option<String> {
        let lang = self.data.lang?.to_639_1()?;

        match translator
            .get_translator_for_lang(lang)?
            .translate(&self.data.text, lang)
            .await
            .context(here!())
        {
            Ok(tl) => Some(tl),
            Err(e) => {
                error!("{:?}", e);
                None
            }
        }
    }

    fn schedule_update(&self, talent: &Talent) -> Option<ScheduleUpdate> {
        let keyword = talent.schedule_keyword.as_ref()?;
        let includes = self.includes.as_ref()?;

        if includes.media.is_empty()
            || !self
                .data
                .text
                .to_lowercase()
                .contains(&keyword.to_lowercase())
        {
            return None;
        }

        let schedule_image = match &includes.media[..] {
            [media, ..] => match media.url.as_ref() {
                Some(url) => url.to_string(),
                None => {
                    warn!("Detected schedule image had no URL.");
                    return None;
                }
            },
            [] => {
                warn!("Detected schedule post didn't include image!");
                return None;
            }
        };

        return Some(ScheduleUpdate {
            twitter_id: self.data.author_id.unwrap().0,
            tweet_text: self.data.text.clone(),
            schedule_image,
            tweet_link: format!(
                "https://twitter.com/{}/status/{}",
                talent.twitter_handle.as_ref().unwrap(),
                self.data.id
            ),
            timestamp: self.data.created_at.unwrap(),
        });
    }

    fn talent_reply(&self, talents: &[Talent]) -> Option<HoloTweetReference> {
        let reference = self.data.referenced_tweets.first()?;

        let replied_to_user = match &reference.reply_type {
            twitter::TweetReferenceType::RepliedTo => self.data.in_reply_to_user_id?,
            twitter::TweetReferenceType::Quoted => {
                self.includes
                    .as_ref()?
                    .tweets
                    .iter()
                    .find(|t| t.id == reference.id)?
                    .author_id?
            }
            _ => {
                warn!(reply_type = ?reference.reply_type, "Unknown reply type");
                return None;
            }
        };

        if talents
            .iter()
            .any(|u| matches!(u.twitter_id, Some(id) if id == replied_to_user.0))
        {
            Some(HoloTweetReference {
                user: replied_to_user.0,
                tweet: reference.id.0,
            })
        } else {
            // If tweet is replying to someone who is not a Hololive talent, don't show the tweet.
            None
        }
    }
}

pub struct TwitterApi;

impl TwitterApi {
    #[instrument(skip(config, notifier_sender, exit_receiver))]
    pub async fn start(
        config: Arc<Config>,
        notifier_sender: Sender<DiscordMessageData>,
        exit_receiver: watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let rules =
            Self::create_talent_rules(config.talents.iter().filter(|t| t.twitter_id.is_some()));

        let stream = FilteredStream::new(
            &config.twitter.token,
            rules,
            FilteredStreamParameters {
                expansions: vec![
                    RequestedExpansion::AttachedMedia,
                    RequestedExpansion::ReferencedTweet,
                ],
                media_fields: vec![MediaField::Url],
                tweet_fields: vec![
                    TweetField::AuthorId,
                    TweetField::CreatedAt,
                    TweetField::Lang,
                    TweetField::InReplyToUserId,
                    TweetField::ReferencedTweets,
                ],
                ..Default::default()
            },
        )
        .await?;

        tokio::spawn(
            async move {
                match Self::tweet_handler(
                    config.twitter.clone(),
                    &config.talents,
                    stream,
                    notifier_sender,
                    config.updates.as_ref().unwrap().subscribe(),
                    exit_receiver,
                )
                .await
                {
                    Ok(_) => (),
                    Err(e) => {
                        error!("{:?}", e);
                    }
                }
            }
            .instrument(debug_span!("Twitter handler")),
        );

        Ok(())
    }

    #[instrument(skip(
        config,
        talents,
        stream,
        notifier_sender,
        config_updates,
        exit_receiver
    ))]
    async fn tweet_handler(
        mut config: TwitterConfig,
        talents: &[Talent],
        mut stream: FilteredStream,
        notifier_sender: Sender<DiscordMessageData>,
        mut config_updates: broadcast::Receiver<ConfigUpdate>,
        mut exit_receiver: watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let mut translator = TranslationApi::new(&config.feed_translation)?;

        loop {
            tokio::select! {
                Some(msg) = stream.next() => {
                    match Self::process_tweet(msg, talents, &translator).await {
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

    async fn process_tweet(
        tweet: twitter::Tweet,
        talents: &[Talent],
        translator: &TranslationApi,
    ) -> anyhow::Result<Option<DiscordMessageData>> {
        let author = match tweet.data.author_id {
            Some(a) => a,
            None => return Ok(None),
        };

        // Find who made the tweet.
        let talent = talents
            .iter()
            .find(|u| u.twitter_id.unwrap() == author.0)
            .ok_or_else(|| anyhow!("Could not find user with twitter ID: {}", author))
            .context(here!())?;

        trace!(talent = %talent.english_name, "Found talent who sent tweet.");

        // Check for schedule keyword.
        if let Some(schedule_update) = tweet.schedule_update(talent) {
            info!("New schedule update from {}.", talent.english_name);
            return Ok(Some(DiscordMessageData::ScheduleUpdate(schedule_update)));
        }

        // Check if we're replying to another talent.
        let replied_to = if !tweet.data.referenced_tweets.is_empty() {
            if let Some(reply) = tweet.talent_reply(talents) {
                Some(reply)
            } else {
                return Ok(None);
            }
        } else {
            None
        };

        // Add attachments if they exist.
        let media = tweet.attached_photos().map(|p| p.to_owned()).collect();

        // Check if translation is necessary.
        let translation = tweet.translate(translator).await;

        info!("New tweet from {}.", talent.english_name);

        Ok(Some(DiscordMessageData::Tweet(HoloTweet {
            id: tweet.data.id.0,
            user: <config::Talent as Clone>::clone(talent),
            text: tweet.data.text,
            link: format!(
                "https://twitter.com/{}/status/{}",
                talent.twitter_handle.as_ref().unwrap(),
                tweet.data.id
            ),
            timestamp: tweet.data.created_at.unwrap(),
            media,
            translation,
            replied_to,
        })))
    }

    fn create_talent_rules<'a, It: Iterator<Item = &'a Talent>>(talents: It) -> Vec<Rule> {
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

        talents
            .map(|t| format!("{}{}", ID_PREFIX, t.twitter_id.unwrap()))
            .chunks(MAX_IDS_PER_RULE)
            .into_iter()
            .enumerate()
            .map(|(i, mut chunk)| Rule {
                value: format!(
                    "{}{}{}",
                    RULE_PREFIX,
                    chunk.join(RULE_SEPARATOR),
                    RULE_SUFFIX
                ),
                tag: format!("Hololive Talents #{}", i + 1),
            })
            .collect::<Vec<_>>()
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
