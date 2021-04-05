use serenity::{
    client::Context,
    model::{
        guild::Guild,
        interactions::{
            ApplicationCommand, Interaction, InteractionApplicationCommandCallbackDataFlags,
            InteractionResponseType,
        },
    },
};

pub async fn setup_interaction(
    ctx: &Context,
    guild: &Guild,
    app_id: u64,
) -> anyhow::Result<ApplicationCommand> {
    let cmd = Interaction::create_guild_application_command(&ctx.http, guild.id, app_id, |i| {
        i.name("ogey").description("rrat")
    })
    .await?;

    Ok(cmd)
}

pub async fn on_interaction(ctx: &Context, interaction: &Interaction) -> anyhow::Result<()> {
    Interaction::create_interaction_response(&interaction, &ctx.http, |r| {
        r.kind(InteractionResponseType::ChannelMessageWithSource)
            .interaction_response_data(|d| {
                d.content("rrat <:pekoSlurp:824792426530734110>")
                    .flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
            })
    })
    .await?;

    Ok(())
}
