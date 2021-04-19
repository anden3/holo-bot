use super::prelude::*;

/*
#[command]
/// rrat
pub async fn ogey(ctx: &Context, msg: &Message) -> CommandResult {
    msg.channel_id
        .say(
            &ctx.http,
            MessageBuilder::new()
                .push("rrat <:pekoSlurp:824792426530734110>")
                .build(),
        )
        .await?;

    Ok(())
}
*/

#[slash_setup]
pub async fn setup(ctx: &Ctx, guild: &Guild, app_id: u64) -> anyhow::Result<ApplicationCommand> {
    let cmd = Interaction::create_guild_application_command(&ctx.http, guild.id, app_id, |i| {
        i.name("ogey").description("rrat")
    })
    .await
    .context(here!())?;

    Ok(cmd)
}

#[slash_command]
pub async fn ogey(ctx: &Ctx, interaction: &Interaction) -> anyhow::Result<()> {
    Interaction::create_interaction_response(interaction, &ctx.http, |r| {
        r.kind(InteractionResponseType::ChannelMessageWithSource)
            .interaction_response_data(|d| {
                d.content("rrat <:pekoSlurp:824792426530734110>")
                    .flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
            })
    })
    .await
    .context(here!())?;

    Ok(())
}
