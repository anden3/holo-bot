use rand::prelude::SliceRandom;

use super::prelude::*;

const RESPONSES: &[&str] = &[
    "As I see it, yes peko.",
    "Ask again later peko.",
    "Better not tell you now peko.",
    "Cannot predict now peko.",
    "Concentrate and ask again peko.",
    "Don’t count on it peko.",
    "It is certain peko.",
    "It is decidedly so peko.",
    "Most likely peko.",
    "My reply is no peko.",
    "My sources say no peko.",
    "Outlook not so good peko.",
    "Outlook good peko.",
    "Reply hazy, try again peko.",
    "Signs point to yes peko.",
    "Very doubtful peko.",
    "Without a doubt peko.",
    "Yes peko.",
    "Yes – definitely peko.",
    "You may rely on it peko.",
];

interaction_setup! {
    name = "eightball",
    group = "fun",
    description = "Roll an 8-ball peko",
    options = [
        //! Which yes/no question do you wish to ask?
        req query: String,
    ],
    restrictions = [
        rate_limit = 1 in 1 minute for user,
        allowed_roles = [
            "Admin",
            "Moderator",
            "Moderator (JP)",
            "20 m deep",
            "30 m deep",
            "40 m deep",
            "50 m deep",
            "60 m deep",
            "70 m deep"
        ]
    ]
}

#[interaction_cmd]
pub async fn eightball(
    ctx: &Ctx,
    interaction: &Interaction,
    config: &Config,
) -> anyhow::Result<()> {
    parse_interaction_options!(
        interaction.data.as_ref().unwrap(), [
        query: req String,
    ]);

    let response = RESPONSES
        .choose(&mut rand::thread_rng())
        .ok_or_else(|| anyhow!("Couldn't pick a response!"))
        .context(here!())?;

    Interaction::create_interaction_response(interaction, &ctx.http, |r| {
        r.kind(InteractionResponseType::ChannelMessageWithSource)
            .interaction_response_data(|d| {
                d.create_embed(|e| {
                    e.title(response).author(|a| {
                        a.name(query).icon_url(
                            "https://images.emojiterra.com/openmoji/v12.2/512px/1f3b1.png",
                        )
                    })
                })
            })
    })
    .await
    .context(here!())?;

    Ok(())
}
