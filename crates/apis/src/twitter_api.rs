use std::{convert::TryInto, sync::Arc};

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use chrono::prelude::*;
use futures::StreamExt;
use tokio::sync::{broadcast, mpsc::Sender};
use tracing::{error, info, instrument, trace, warn};
use twitter::{streams::FilteredStream, Rule, StreamParameters, Tweet};

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
    fn convert_entities_to_links(&self) -> String;
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

    fn convert_entities_to_links(&self) -> String {
        let entities = self.data.entities.iter().filter(|e| {
            matches!(
                e,
                twitter::Entity::Hashtag { .. } | twitter::Entity::Url { .. }
            )
        });

        let mut text = self.data.text.clone();

        for entity in entities {
            entity.embed_link(&mut text);
        }

        text
    }
}

pub struct TwitterApi;

impl TwitterApi {
    #[instrument(skip(config, notifier_sender))]
    pub async fn start(
        config: Arc<Config>,
        notifier_sender: Sender<DiscordMessageData>,
    ) -> anyhow::Result<()> {
        tokio::spawn(async move {
            match Self::tweet_handler(
                config.twitter.clone(),
                &config.talents,
                notifier_sender,
                config.updates.as_ref().unwrap().subscribe(),
            )
            .await
            {
                Ok(_) => (),
                Err(e) => {
                    error!("{:?}", e);
                }
            }
        });

        Ok(())
    }

    #[instrument(skip(config, talents, notifier_sender, config_updates))]
    async fn tweet_handler(
        mut config: TwitterConfig,
        talents: &[Talent],
        notifier_sender: Sender<DiscordMessageData>,
        mut config_updates: broadcast::Receiver<ConfigUpdate>,
    ) -> anyhow::Result<()> {
        use twitter::{MediaField as MF, RequestedExpansion as RE, TweetField as TF};

        let mut translator = TranslationApi::new(&config.feed_translation)?;
        let rules = Self::create_talent_rules(talents.iter().filter(|t| t.twitter_id.is_some()))?;

        let mut stream = FilteredStream::new(
            &config.token,
            StreamParameters {
                expansions: vec![RE::AttachedMedia, RE::ReferencedTweet],
                media_fields: vec![MF::Url],
                tweet_fields: vec![
                    TF::AuthorId,
                    TF::CreatedAt,
                    TF::Lang,
                    TF::InReplyToUserId,
                    TF::ReferencedTweets,
                    TF::Entities,
                ],
                ..Default::default()
            },
        )
        .await?;

        stream.set_rules(rules).await?;

        loop {
            tokio::select! {
                Some(tweet) = stream.next() => {
                    trace!(?tweet, "Tweet received!");

                    match Self::process_tweet(tweet, talents, &translator).await {
                        Ok(Some(discord_message)) => {
                            trace!(update = ?discord_message, "Tweet update detected!");
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

                res = tokio::signal::ctrl_c() => {
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

        trace!(talent = %talent.name, "Found talent who sent tweet.");

        // Check for schedule keyword.
        if let Some(schedule_update) = tweet.schedule_update(talent) {
            info!("New schedule update from {}.", talent.name);
            return Ok(Some(DiscordMessageData::ScheduleUpdate(schedule_update)));
        }

        // Check if we're replying to another talent.
        let replied_to = if !tweet.data.referenced_tweets.is_empty() {
            tweet.talent_reply(talents)
        } else {
            None
        };

        // Add attachments if they exist.
        let media = tweet.attached_photos().map(|p| p.to_owned()).collect();

        // Check if translation is necessary.
        let translation = tweet.translate(translator).await;

        info!("New tweet from {}.", talent.name);

        Ok(Some(DiscordMessageData::Tweet(HoloTweet {
            id: tweet.data.id.0,
            user: <config::Talent as Clone>::clone(talent),
            text: tweet.convert_entities_to_links(),
            // text: tweet.data.text,
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

    fn create_talent_rules<'a, It: Iterator<Item = &'a Talent>>(
        talents: It,
    ) -> Result<Vec<Rule>, twitter::Error> {
        const RULE_SUFFIX: &str = "-is:retweet";
        const RULE_SEPARATOR: &str = " OR ";
        const ID_PREFIX: &str = "from:";
        const GROUPING_LENGTH: usize = "() ".len();

        const RULE_MAX_LEN: usize = FilteredStream::MAX_RULE_LENGTH;
        const ID_MAX_LEN: usize = 20;

        const ID_WITH_PREFIX_LEN: usize = ID_MAX_LEN + ID_PREFIX.len();
        const RULE_MAX_LEN_WITHOUT_FIXES: usize =
            RULE_MAX_LEN - RULE_SUFFIX.len() - GROUPING_LENGTH;

        const MAX_IDS_PER_RULE: usize = (RULE_MAX_LEN_WITHOUT_FIXES + RULE_SEPARATOR.len())
            / (ID_WITH_PREFIX_LEN + RULE_SEPARATOR.len());

        debug_assert_eq!(ID_MAX_LEN, u64::MAX.to_string().len());

        talents
            .map(|t| format!("{}{}", ID_PREFIX, t.twitter_id.unwrap()))
            .collect::<Vec<String>>()
            .chunks(MAX_IDS_PER_RULE)
            .into_iter()
            .enumerate()
            .map(|(i, chunk)| {
                let value = if chunk.len() == 1 {
                    format!("{} {}", chunk[0], RULE_SUFFIX)
                } else {
                    format!("({}) {}", chunk.join(RULE_SEPARATOR), RULE_SUFFIX)
                };

                Ok(Rule {
                    value: value.try_into()?,
                    tag: format!("Hololive Talents #{}", i + 1),
                })
            })
            .collect::<Result<Vec<_>, _>>()
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
