use super::prelude::*;

interaction_setup! {
    name = "watching",
    description = "Shows which channels are watching which streams."
}

#[interaction_cmd]
pub async fn watching(ctx: &Ctx, interaction: &Interaction) -> anyhow::Result<()> {
    show_deferred_response(&interaction, &ctx, false).await?;
    let app_id = *ctx.cache.current_user_id().await.as_u64();

    let data = ctx.data.read().await;
    let claimed_channels = data.get::<ClaimedChannels>().unwrap();

    if claimed_channels.is_empty() {
        interaction
            .delete_original_interaction_response(&ctx.http, app_id)
            .await?;
        return Ok(());
    }

    if let Some((stream, _)) = claimed_channels.get(&interaction.channel_id) {
        interaction
            .edit_original_interaction_response(&ctx.http, app_id, |e| {
                e.embed(|e| {
                    e.title("Now watching")
                        .description(&stream.title)
                        .url(format!("https://youtube.com/watch?v={}", stream.url))
                        .timestamp(&stream.start_at)
                        .colour(stream.streamer.colour)
                        .thumbnail(format!(
                            "https://i3.ytimg.com/vi/{}/maxresdefault.jpg",
                            stream.url
                        ))
                        .author(|a| {
                            a.name(&stream.streamer.display_name)
                                .url(format!(
                                    "https://www.youtube.com/channel/{}",
                                    stream.streamer.channel
                                ))
                                .icon_url(&stream.streamer.icon)
                        })
                })
            })
            .await?;
    } else {
        interaction
            .edit_original_interaction_response(&ctx.http, app_id, |e| {
                e.embed(|e| {
                    e.title("Channels currently being used for streams.")
                        .field(
                            "Channel",
                            claimed_channels
                                .iter()
                                .fold(String::new(), |acc, (id, _)| {
                                    acc + &format!("{}\r\n", Mention::from(*id))
                                })
                                .trim_end(),
                            true,
                        )
                        .field(
                            "Talent",
                            claimed_channels
                                .iter()
                                .fold(String::new(), |acc, (_, (s, _))| {
                                    acc + &format!(
                                        "{}\r\n",
                                        Mention::from(RoleId(s.streamer.discord_role))
                                    )
                                })
                                .trim_end(),
                            true,
                        )
                        .field(
                            "Stream",
                            claimed_channels
                                .iter()
                                .fold(String::new(), |acc, (_, (s, _))| {
                                    acc + &format!(
                                        "[{}](https://youtube.com/watch?v={})\r\n",
                                        s.title, s.url
                                    )
                                })
                                .trim_end(),
                            true,
                        )
                })
            })
            .await?;
    }

    Ok(())
}
