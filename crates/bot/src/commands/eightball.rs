use nanorand::Rng;

use super::prelude::*;

static RESPONSES: &[&str] = &[
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

#[poise::command(
    slash_command,
    prefix_command,
    rename = "8ball",
    required_permissions = "SEND_MESSAGES",
    member_cooldown = 60
)]
/// Roll an 8-ball, peko.
pub(crate) async fn eightball(
    ctx: Context<'_>,
    #[description = "Which yes/no question do you wish to ask?"] question: String,
) -> anyhow::Result<()> {
    let response = { RESPONSES[nanorand::tls_rng().generate_range(0..RESPONSES.len())] };

    ctx.send(|m| {
        m.embed(|e| {
            e.title(response).author(|a| {
                a.name(question)
                    .icon_url("https://images.emojiterra.com/openmoji/v12.2/512px/1f3b1.png")
            })
        })
    })
    .await?;

    Ok(())
}
