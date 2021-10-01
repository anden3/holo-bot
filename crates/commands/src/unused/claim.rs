use chrono::Utc;

use apis::holo_api::{Livestream, StreamState, StreamUpdate};
use utility::config::User;

use super::prelude::*;

interaction_setup! {
    name = "claim",
    description = "Claims the channel for a specific talent.",
    options = [
        //! The talent to claim the channel for.
        req talent: String,
    ],
    restrictions = [
        allowed_roles = [
            "Admin",
            "Moderator",
            "Moderator (JP)",
            824337391006646343,
        ]
    ]
}

#[interaction_cmd]
pub async fn claim(
    ctx: &Ctx,
    interaction: &Interaction,
    config: &Config,
    app_id: u64,
) -> anyhow::Result<()> {
    parse_interaction_options!(
        interaction.data.as_ref().unwrap(), [
        talent: req String,
    ]);
    show_deferred_response(&interaction, &ctx, false).await?;

    // Make sure channel isn't already claimed.
    {
        let data = ctx.data.read().await;
        let claimed_channels = data.get::<ClaimedChannels>().unwrap();

        if claimed_channels.contains_key(&interaction.channel_id) {
            interaction
                .edit_original_interaction_response(&ctx.http, app_id, |e| {
                    e.content("Channel is already claimed!")
                })
                .await?;

            return Ok(());
        }
    }

    let user = {
        match config.users.iter().find(|u| {
            u.english_name
                .to_lowercase()
                .contains(&talent.trim().to_lowercase())
        }) {
            Some(u) => u.clone(),
            None => {
                let mut user = None;

                if let Some(role) = serenity::utils::parse_role(&talent.trim()) {
                    if let Some(u) = config.users.iter().find(|u| u.discord_role == role) {
                        user = Some(u);
                    }
                }

                match user {
                    Some(u) => u.clone(),
                    None => {
                        interaction
                            .edit_original_interaction_response(&ctx.http, app_id, |e| {
                                e.content(format!("No talent with the name {} found.", talent))
                            })
                            .await?;

                        return Ok(());
                    }
                }
            }
        }
    };

    let matching_stream = {
        match get_matching_stream(&ctx, &user).await {
            Some(s) => s,
            None => {
                interaction
                    .edit_original_interaction_response(&ctx.http, app_id, |e| {
                        e.content(format!("{} is not streaming right now.", user.english_name))
                            .allowed_mentions(|m| m.empty_parse())
                    })
                    .await?;

                return Ok(());
            }
        }
    };

    let mut stream_update = ctx
        .data
        .read()
        .await
        .get::<StreamUpdateTx>()
        .unwrap()
        .subscribe();

    let mut channel = match interaction.channel_id.to_channel(&ctx.http).await? {
        Channel::Guild(c) => c,
        _ => anyhow::bail!("Wrong channel type!"),
    };

    let old_channel_name = channel.name.clone();
    let old_channel_desc = channel.topic.clone().unwrap_or_default();

    let new_name = format!(
        "{}-{}-stream",
        user.emoji,
        user.english_name.to_ascii_lowercase().replace(' ', "-")
    );
    let new_desc = format!("https://youtube.com/watch?v={}", matching_stream.url);

    channel
        .edit(&ctx.http, |c| c.name(new_name).topic(new_desc))
        .await
        .context(here!())?;

    info!(name = %channel.name, desc = ?channel.topic, "Channel edited!");

    interaction
        .edit_original_interaction_response(&ctx.http, app_id, |e| {
            e.embed(|e| {
                e.title("Now watching")
                    .description(&matching_stream.title)
                    .url(format!(
                        "https://youtube.com/watch?v={}",
                        matching_stream.url
                    ))
                    .timestamp(&matching_stream.start_at)
                    .colour(user.colour)
                    .image(format!(
                        "https://i3.ytimg.com/vi/{}/maxresdefault.jpg",
                        matching_stream.url
                    ))
                    .author(|a| {
                        a.name(&user.english_name)
                            .url(format!("https://www.youtube.com/channel/{}", user.channel))
                            .icon_url(&user.icon)
                    })
            })
        })
        .await?;

    let token = CancellationToken::new();
    let child_token = token.child_token();

    // Register channel as claimed.
    {
        let mut data = ctx.data.write().await;
        let claimed_channels = data.get_mut::<ClaimedChannels>().unwrap();

        claimed_channels.insert(channel.id, (matching_stream.clone(), token));
    }

    loop {
        tokio::select! {
            _ = child_token.cancelled() => {
                info!("Claim cancelled!");
                break;
            }
            Ok(update) = stream_update.recv() => {
                if let StreamUpdate::Ended(s) = update {
                    if s == matching_stream {
                        info!("Claim expired!");
                        break;
                    }
                }
            }
        }
    }

    // Remove claim.
    {
        let mut data = ctx.data.write().await;
        let claimed_channels = data.get_mut::<ClaimedChannels>().unwrap();

        claimed_channels.remove(&channel.id);
        info!("Claim removed!");
    }

    // Restore channel.
    channel
        .edit(&ctx.http, |c| {
            c.name(old_channel_name).topic(old_channel_desc)
        })
        .await
        .context(here!())?;

    info!(name = %channel.name, desc = ?channel.topic, "Channel edited!");

    if !child_token.is_cancelled() {
        channel
            .send_message(&ctx.http, |m| {
                m.embed(|e| {
                    e.title("Stream ended")
                        .description(&matching_stream.title)
                        .colour(user.colour)
                        .footer(|f| f.text(format!("Stream ended at {}.", Utc::now())))
                        .image(format!(
                            "https://i3.ytimg.com/vi/{}/maxresdefault.jpg",
                            matching_stream.url
                        ))
                        .author(|a| {
                            a.name(&user.english_name)
                                .url(format!("https://www.youtube.com/channel/{}", user.channel))
                                .icon_url(&user.icon)
                        })
                })
            })
            .await?;
    }

    Ok(())
}

async fn get_matching_stream(ctx: &Ctx, user: &User) -> Option<Livestream> {
    let data = ctx.data.read().await;
    let streams = data.get::<StreamIndex>().unwrap().borrow();

    let matching_stream = streams
        .iter()
        .map(|(_, s)| s)
        .find(|s| s.state == StreamState::Live && s.streamer == *user);

    matching_stream.cloned()
}
