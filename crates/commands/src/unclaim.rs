use super::prelude::*;

interaction_setup! {
    name = "unclaim",
    description = "Removes claim on this channel.",
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
pub async fn unclaim(ctx: &Ctx, interaction: &Interaction) -> anyhow::Result<()> {
    show_deferred_response(&interaction, &ctx, true).await?;

    let app_id = *ctx.cache.current_user_id().await.as_u64();

    let data = ctx.data.read().await;
    let claimed_channels = data.get::<ClaimedChannels>().unwrap();

    if let Some((_stream, token)) = claimed_channels.get(&interaction.channel_id) {
        token.cancel();
        interaction
            .edit_original_interaction_response(&ctx.http, app_id, |e| {
                e.content("Channel unclaimed!")
            })
            .await?;
    } else {
        interaction
            .edit_original_interaction_response(&ctx.http, app_id, |e| {
                e.content("Channel was not claimed!")
            })
            .await?;
    }

    Ok(())
}
