use super::prelude::*;

interaction_setup! {
    name = "donate",
    group = "utility",
    description = "Support me peko!"
}

#[interaction_cmd]
pub async fn donate(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    config: &Config,
) -> anyhow::Result<()> {
    interaction
        .create_interaction_response(&ctx.http, |r| {
            r.kind(InteractionResponseType::ChannelMessageWithSource)
                .interaction_response_data(|d| {
                    d.flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
                        .create_embed(|e| e
                            .title("Donation Information")
                            .colour(Colour::from_rgb(0xEC, 0x9C, 0xFC))
                            .description(
                                "*Almondo, almondo peko!*\n\n\
                                If you are interested in helping support my development, \
                                and invest in better hosting, we'd appreciate your support peko!\n\n\
                                Any amount is appreciated, and all donations will go directly towards development \
                                and new hardware peko!")
                            .field(
                                "Links", 
                                "Donations can be made via either [GitHub Sponsors](https://github.com/sponsors/anden3) \
                                or [Ko-Fi](https://ko-fi.com/anden3) peko! \
                                Any amount is greatly appreciated peko!", false)
                            .field(
                                "Disclaimer",
                                "No donations will ever be required to access any features of the bot, \
                                so if you feel like you can't spare some extra money, please save it for yourself peko. \
                                Additionally, please consider that all donations are non-refundable peko.",
                                false)
                            .footer(|f| f.text("I am made by anden3#0003 peko!"))
                        )
                })
        })
        .await
        .context(here!())?;

    Ok(())
}
