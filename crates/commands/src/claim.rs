use apis::holo_api::{HoloApi, StreamState, StreamUpdate};

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
            "Moderator (JP)"
        ]
    ]
}

#[interaction_cmd]
pub async fn claim(ctx: &Ctx, interaction: &Interaction) -> anyhow::Result<()> {
    parse_interaction_options!(
        interaction.data.as_ref().unwrap(), [
        talent: req String,
    ]);
    show_deferred_response(&interaction, &ctx).await?;

    let app_id = *ctx.cache.current_user_id().await.as_u64();

    let user = {
        let data = ctx.data.read().await;
        let config = data.get::<Config>().unwrap();

        match config.users.iter().find(|u| {
            u.display_name
                .to_lowercase()
                .contains(&talent.trim().to_lowercase())
        }) {
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
    };

    let matching_stream = {
        let streams = match HoloApi::read_stream_index() {
            Some(index) => index.read().await,
            None => anyhow::bail!("Stream index wasn't initialized!"),
        };

        let matching_stream = streams
            .iter()
            .map(|(_, s)| s)
            .find(|s| s.state == StreamState::Live && s.streamer == user);

        match matching_stream {
            Some(s) => s.clone(),
            None => {
                interaction
                    .edit_original_interaction_response(&ctx.http, app_id, |e| {
                        e.content(format!("{} is not streaming right now.", user.display_name))
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
        user.display_name.to_ascii_lowercase().replace(' ', "-")
    );
    let new_desc = format!("https://youtube.com/watch?v={}", matching_stream.url);

    channel
        .edit(&ctx.http, |c| c.name(new_name).topic(new_desc))
        .await
        .context(here!())?;

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
                        "https://img.youtube.com/vi/{}/hqdefault.jpg",
                        matching_stream.url
                    ))
                    .author(|a| {
                        a.name("Channel claimed");
                        a.url(format!("https://www.youtube.com/channel/{}", user.channel));
                        a.icon_url(&user.icon);

                        a
                    })
            })
        })
        .await?;

    while let Ok(update) = stream_update.recv().await {
        if let StreamUpdate::Ended(s) = update {
            if s == matching_stream {
                break;
            }
        }
    }

    channel
        .edit(&ctx.http, |c| {
            c.name(old_channel_name).topic(old_channel_desc)
        })
        .await
        .context(here!())?;

    Ok(())
}
