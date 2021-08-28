use regex::Regex;
use serenity::{
    builder::{CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter, CreateMessage},
    model::channel::{Embed, EmbedAuthor, EmbedField, EmbedFooter},
};

use super::prelude::*;

use utility::regex_lazy;

static SENTENCE_RGX: once_cell::sync::Lazy<Regex> = regex_lazy!(
    r#"(?msx)                                                               # Flags
        (?P<text>.*?[\w&&[^_]]+.*?)                                             # Text, not including underscores at the end.
        (?P<punct>
            [\.!\?\u3002\u0629\uFE12\uFE52\uFF0E\uFF61\uFF01\uFF1F"_\*`\)]+     # Match punctuation not at the end of a line.
            |
        \s*(?:                                                                  # Include eventual whitespace after peko.
                [\.!\?\u3002\u0629\uFE12\uFE52\uFF0E\uFF61\uFF01\uFF1F"_\*`\)]  # Match punctuation at the end of a line.
                |
                (?:<a?:\w+:\d+>)                                                # Match Discord emotes at the end of a line.
                |
                [\x{1F600}-\x{1F64F}]                                           # Match Unicode emoji at the end of a line.
            )*$
        )"#
);

#[command]
#[allowed_roles(
    "Admin",
    "Moderator",
    "Moderator (JP)",
    "Server Booster",
    "40 m deep",
    "50 m deep",
    "60 m deep",
    "70 m deep",
    "80 m deep",
    "90 m deep",
    "100 m deep"
)]
/// Pekofies replied-to message or the provided text.
pub async fn pekofy(ctx: &Ctx, msg: &Message) -> CommandResult {
    let mut reply = CreateMessage::default();

    let (text, embeds) = match get_data(ctx, msg).await? {
        Some((text, embeds)) => (text, embeds),
        None => return Ok(()),
    };

    let mut result = Ok(());

    if let Some(text) = text {
        result = result.and(if text.starts_with("-pekofy") {
            Err(anyhow!("Nice try peko"))
        } else {
            pekofy_text(&text).map(|text| {
                reply.content(text);
            })
        });
    }

    if !embeds.is_empty() {
        result = result.and(pekofy_embeds(msg, &mut reply).await);
    }

    if let Err(e) = result {
        msg.channel_id
            .say(&ctx.http, e.to_string())
            .await
            .context(here!())?;

        return Ok(());
    }

    msg.channel_id
        .send_message(&ctx.http, |_| &mut reply)
        .await
        .context(here!())?;

    Ok(())
}

fn pekofy_text(text: &str) -> anyhow::Result<String> {
    let mut pekofied_text = String::with_capacity(text.len());

    for capture in SENTENCE_RGX.captures_iter(text) {
        // Check if the capture is empty.
        if capture
            .get(0)
            .map(|c| c.as_str().trim().is_empty())
            .unwrap_or(true)
        {
            continue;
        }

        let text = capture
            .name("text")
            .ok_or_else(|| anyhow!("Couldn't find 'text' capture!"))
            .context(here!())?
            .as_str();

        let response = get_peko_response(text)?;

        capture.expand(&format!("$text{}$punct", response), &mut pekofied_text);
    }

    Ok(pekofied_text)
}

async fn pekofy_embeds(msg: &Message, reply: &mut CreateMessage<'_>) -> anyhow::Result<()> {
    reply.set_embeds(
        msg.embeds
            .iter()
            .map(|e| pekofy_embed(e))
            .collect::<anyhow::Result<_>>()?,
    );

    Ok(())
}

fn pekofy_embed(embed: &Embed) -> anyhow::Result<CreateEmbed> {
    let mut peko_embed = CreateEmbed::default();

    if let Some(EmbedAuthor {
        name,
        icon_url,
        url,
        ..
    }) = &embed.author
    {
        let mut peko_author = CreateEmbedAuthor::default();

        peko_author.name(pekofy_text(name)?);

        if let Some(icon_url) = icon_url {
            peko_author.icon_url(icon_url);
        }

        if let Some(url) = url {
            peko_author.url(url);
        }

        peko_embed.set_author(peko_author);
    }

    if let Some(EmbedFooter { text, icon_url, .. }) = &embed.footer {
        let mut peko_footer = CreateEmbedFooter::default();

        peko_footer.text(pekofy_text(text)?);

        if let Some(icon_url) = icon_url {
            peko_footer.icon_url(icon_url);
        }

        peko_embed.set_footer(peko_footer);
    }

    if let Some(title) = &embed.title {
        peko_embed.title(pekofy_text(title)?);
    }

    if let Some(description) = &embed.description {
        peko_embed.description(pekofy_text(description)?);
    }

    if !embed.fields.is_empty() {
        peko_embed.fields(
            embed
                .fields
                .iter()
                .map(
                    |EmbedField {
                         name,
                         value,
                         inline,
                         ..
                     }| {
                        match [pekofy_text(name), pekofy_text(value)] {
                            [Ok(name), Ok(value)] => Ok((name, value, *inline)),
                            [Err(n), Err(v)] => Err(n).context(v),
                            [Err(e), _] | [_, Err(e)] => Err(e),
                        }
                    },
                )
                .collect::<anyhow::Result<Vec<_>>>()?,
        );
    }

    Ok(peko_embed)
}

#[allow(clippy::needless_lifetimes)]
async fn get_data<'a>(
    ctx: &Ctx,
    msg: &'a Message,
) -> anyhow::Result<Option<(Option<String>, &'a Vec<Embed>)>> {
    let mut args = Args::new(
        &msg.content_safe(&ctx.cache).await,
        &[Delimiter::Single(' ')],
    );
    args.trimmed();
    args.advance();

    let text;
    let embeds;

    if let Some(src) = &msg.referenced_message {
        if src.author.bot {
            return Ok(None);
        }

        embeds = &src.embeds;

        let safe_text = src.content_safe(&ctx.cache).await;

        text = if safe_text.trim().is_empty() {
            if embeds.is_empty() {
                return Ok(None);
            }

            None
        } else {
            Some(safe_text)
        };

        msg.delete(&ctx.http).await.context(here!())?;
    } else {
        embeds = &msg.embeds;

        text = match args.remains() {
            Some(remains) => Some(remains.to_owned()),
            None if embeds.is_empty() => return Ok(None),
            None => None,
        };

        msg.delete(&ctx.http).await.context(here!())?;
    }

    Ok(Some((text, embeds)))
}

fn get_peko_response(text: &str) -> anyhow::Result<&str> {
    let text_is_uppercase = text == text.to_uppercase();

    // Get response based on alphabet used.
    Ok(
        match text
            .chars()
            .last()
            .ok_or_else(|| anyhow!("Can't get last character!"))
            .context(here!())? as u32
        {
            // Greek
            0x0370..=0x03FF => {
                if text_is_uppercase {
                    " ΠΈΚΟ"
                } else {
                    " πέκο"
                }
            }
            // Russian
            0x0400..=0x04FF => {
                if text_is_uppercase {
                    " ПЕКО"
                } else {
                    " пеко"
                }
            }
            // Arabic
            0x0600..=0x06FF => "بيكو ",
            // Georgian
            0x10A0..=0x10FF | 0x1C90..=0x1CBF => " პეკო",
            // Japanese
            0x3040..=0x30FF | 0xFF00..=0xFFEF | 0x4E00..=0x9FAF => "ぺこ",
            // Korean
            0xAC00..=0xD7AF
            | 0x1100..=0x11FF
            | 0xA960..=0xA97F
            | 0xD7B0..=0xD7FF
            | 0x3130..=0x318F => "페코",
            // Latin
            _ => {
                if text_is_uppercase {
                    " PEKO"
                } else {
                    " peko"
                }
            }
        },
    )
}
