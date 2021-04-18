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

#[slash_setup]
pub async fn setup(
    ctx: &Context,
    guild: &Guild,
    app_id: u64,
) -> anyhow::Result<ApplicationCommand> {
    let cmd = Interaction::create_guild_application_command(&ctx.http, guild.id, app_id, |i| {
        i.name("eightball")
            .description("Roll an 8-ball peko")
            .create_interaction_option(|o| {
                o.name("query")
                    .description("Which yes/no question do you wish to ask?")
                    .kind(ApplicationCommandOptionType::String)
                    .required(true)
            });
        i
    })
    .await?;

    Ok(cmd)
}

#[slash_command]
#[allowed_roles(
    "Admin",
    "Moderator",
    "Moderator (JP)",
    "Server Booster",
    "20 m deep",
    "30 m deep",
    "40 m deep",
    "50 m deep",
    "60 m deep",
    "70 m deep",
    "80 m deep",
    "90 m deep",
    "100 m deep"
)]
pub async fn eightball(ctx: &Context, interaction: &Interaction) -> anyhow::Result<()> {
    let question = &interaction
        .data
        .as_ref()
        .and_then(|d| d.options.iter().find(|o| o.name == "query"))
        .and_then(|q| q.value.as_ref())
        .ok_or_else(|| anyhow!("Couldn't get question!"))?;

    let response = RESPONSES
        .choose(&mut rand::thread_rng())
        .ok_or_else(|| anyhow!("Couldn't pick a response!"))?;

    Interaction::create_interaction_response(interaction, &ctx.http, |r| {
        r.kind(InteractionResponseType::ChannelMessageWithSource)
            .interaction_response_data(|d| {
                d.embed(|e| {
                    e.title(MessageBuilder::new().push_safe(question).build())
                        .description(MessageBuilder::new().push_bold(response).build())
                        .thumbnail("https://images.emojiterra.com/openmoji/v12.2/512px/1f3b1.png")
                })
            })
    })
    .await?;

    Ok(())
}
