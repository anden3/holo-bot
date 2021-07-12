use super::prelude::*;

interaction_setup! {
    name = "ogey",
    group = "utility",
    description = "rrat"
}

#[interaction_cmd]
pub async fn ogey(ctx: &Ctx, interaction: &Interaction, config: &Config) -> anyhow::Result<()> {
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
