use std::{collections::HashSet, sync::Arc};

use anyhow::Context;
use chrono::Utc;
use futures::stream::{FuturesUnordered, StreamExt};
use serenity::{
    model::{
        channel::ReactionType,
        id::{RoleId, UserId},
        misc::Mention,
    },
    prelude::Mentionable,
    utils::Color,
    CacheAndHttp,
};
use tokio::{select, sync::broadcast, time::sleep};
use tracing::{debug, error, instrument, warn};
use unicode_truncate::UnicodeTruncateStr;
use utility::{config::ReactTempMuteConfig, discord::ReactionUpdate, here};

#[instrument(skip(ctx, config, reactions))]
pub async fn handler(
    ctx: Arc<CacheAndHttp>,
    config: &ReactTempMuteConfig,
    mut reactions: broadcast::Receiver<ReactionUpdate>,
) -> anyhow::Result<()> {
    struct ReactedMessage {
        count: usize,
        reacters: HashSet<UserId>,
    }

    let mut mute_participants: lru::LruCache<UserId, usize> = lru::LruCache::new(16);
    let mut cache = lru::LruCache::new(8);
    let mut muted_users = FuturesUnordered::new();

    loop {
        let reaction;

        select! {
            _ = muted_users.select_next_some(), if !muted_users.is_empty() => {
                debug!("User unmuted!");
                continue;
            }

            r = reactions.recv() => {
                reaction = match r {
                    Ok(r) => r,
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(e) => {
                        error!(?e, "Failed to receive reaction!");
                        continue;
                    }
                }
            }
        }

        let (r, was_removed) = match reaction {
            ReactionUpdate::Added(r) => (r, false),
            ReactionUpdate::Removed(r) => (r, true),
        };

        // Check right emoji.
        match r.emoji {
            ReactionType::Unicode(_) => continue,
            ReactionType::Custom { id, .. } if !config.reactions.contains(&id) => continue,
            _ => (),
        }

        // Check eligibility.
        if Utc::now() - r.message_id.created_at() > config.eligibility_duration {
            cache.pop(&r.message_id);
            continue;
        }

        let user_id = match r.user_id {
            Some(id) => id,
            None => continue,
        };

        let message = match r
            .channel_id
            .message(&ctx.http, &r.message_id)
            .await
            .context(here!())
        {
            Ok(m) => m,
            Err(e) => {
                error!(?e, "Failed to get message!");
                continue;
            }
        };

        // Check if bot.
        if message.author.bot {
            continue;
        }

        // Check permissions.
        let base_permissions = match message.guild_field(&ctx.cache, |g| g.roles.clone()) {
            Some(roles) => match roles.get(&RoleId(message.guild_id.unwrap().0)) {
                Some(r) => r.permissions,
                None => {
                    warn!("No @everyone role found for guild!");
                    continue;
                }
            },
            None => {
                warn!("Could not get guild that message was sent in.");
                continue;
            }
        };

        if !base_permissions.send_messages() {
            continue;
        }

        let mut msg_data = match cache.get_mut(&r.message_id) {
            Some(m) => m,
            None if was_removed => continue,
            None => {
                cache.put(
                    r.message_id,
                    ReactedMessage {
                        count: 0,
                        reacters: HashSet::new(),
                    },
                );

                cache.get_mut(&r.message_id).unwrap()
            }
        };

        if was_removed {
            if !msg_data.reacters.remove(&user_id) {
                continue;
            }

            msg_data.count -= 1;

            if msg_data.count == 0 {
                cache.pop(&r.message_id);
            }

            continue;
        } else {
            if !msg_data.reacters.insert(user_id) {
                continue;
            }

            msg_data.count += 1;
        }

        if msg_data.count >= config.required_reaction_count {
            let msg_data = cache.pop(&r.message_id).unwrap();

            if let Err(e) = message.delete(&ctx.http).await.context(here!()) {
                error!(?e, "Failed to delete message!");
            }

            let guild_id = r.guild_id.unwrap();
            let author_id = message.author.id;
            let http = ctx.http.clone();

            let mute_future = async move {
                let mut member = match guild_id.member(&http, author_id).await.context(here!()) {
                    Ok(m) => m,
                    Err(e) => {
                        error!(?e, "Failed to get member!");
                        return;
                    }
                };

                if let Err(e) = member
                    .add_role(&http, config.mute_role)
                    .await
                    .context(here!())
                {
                    error!(?e, "Failed to mute member!");
                    return;
                }

                sleep(config.mute_duration.to_std().unwrap()).await;

                if let Err(e) = member
                    .remove_role(&http, config.mute_role)
                    .await
                    .context(here!())
                {
                    error!(?e, "Failed to unmute member!");

                    if let Some(log_ch) = config.logging_channel {
                        let _ = log_ch
                            .say(
                                &http,
                                format!(
                                    "@here {} failed to be unmuted, do it manually!",
                                    Mention::from(author_id)
                                ),
                            )
                            .await;
                    }
                }
            };

            muted_users.push(mute_future);

            if let Some(log_ch) = &config.logging_channel {
                let mut content = message.content.clone();

                if content.len() >= 1024 {
                    let (truncated_data, _len) = content.unicode_truncate(1021);
                    content = format!("{}...", truncated_data);
                }

                let mut voters = Vec::with_capacity(msg_data.reacters.len());

                for voter_id in &msg_data.reacters {
                    let voter = match voter_id
                        .to_user(&ctx.http)
                        .await
                        .context(here!())
                        .map(|u| u.tag())
                    {
                        Ok(v) => v,
                        Err(e) => {
                            error!(?e, "Failed to get user!");
                            Mention::from(*voter_id).to_string()
                        }
                    };

                    voters.push(voter)
                }

                let res = log_ch
                    .send_message(&ctx.http, |m| {
                        m.embed(|e| {
                            e.title("User got temp bonked");
                            e.author(|a| {
                                a.name(message.author.tag()).icon_url(
                                    message
                                        .author
                                        .avatar_url()
                                        .unwrap_or_else(|| message.author.default_avatar_url()),
                                )
                            });
                            e.colour(Color::RED);
                            e.fields([
                                ("Channel", message.channel_id.mention().to_string(), true),
                                ("Message", content, true),
                                (
                                    "Voters",
                                    voters
                                        .iter()
                                        .fold(String::new(), |acc, u| format!("{}\n{}", acc, u)),
                                    true,
                                ),
                            ]);

                            if !message.attachments.is_empty() {
                                e.image(message.attachments[0].url.clone());

                                if message.attachments.len() > 1 {
                                    e.field(
                                        "Additional Images",
                                        message
                                            .attachments
                                            .iter()
                                            .skip(1)
                                            .fold(String::new(), |s, i| {
                                                format!("{}\n{}", s, i.url)
                                            }),
                                        true,
                                    );
                                }
                            }

                            e
                        });

                        m
                    })
                    .await;

                if let Err(e) = res.context(here!()) {
                    error!(?e, "Failed to send logging message!");
                }
            }

            // Keep track of users muting others often.
            for user in msg_data.reacters {
                if let Some(mute_count) = mute_participants.get_mut(&user) {
                    *mute_count += 1;

                    if *mute_count >= config.excessive_mute_threshold {
                        if let Some(log_ch) = &config.logging_channel {
                            let _ = log_ch
                                .say(
                                    &ctx.http,
                                    format!(
                                        "{} has helped mute people {} times as of late.",
                                        Mention::from(user),
                                        mute_count
                                    ),
                                )
                                .await;
                        }
                    }
                } else {
                    mute_participants.put(user, 1);
                }
            }
        }
    }

    Ok(())
}
