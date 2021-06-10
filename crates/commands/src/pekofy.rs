use regex::Regex;

use super::prelude::*;

use utility::regex;

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
    let sentence_rgx: &'static Regex = regex!(
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

    let text = match get_text(ctx, msg).await? {
        Some(t) => t,
        None => return Ok(()),
    };

    if text.starts_with("-pekofy") {
        msg.channel_id
            .say(&ctx.http, "Nice try peko")
            .await
            .context(here!())?;
        return Ok(());
    }

    let mut pekofied_text = String::with_capacity(text.len());

    for capture in sentence_rgx.captures_iter(&text) {
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

    if pekofied_text.trim().is_empty() {
        return Ok(());
    }

    msg.channel_id
        .say(&ctx.http, pekofied_text)
        .await
        .context(here!())?;

    Ok(())
}

async fn get_text(ctx: &Ctx, msg: &Message) -> anyhow::Result<Option<String>> {
    let mut args = Args::new(
        &msg.content_safe(&ctx.cache).await,
        &[Delimiter::Single(' ')],
    );
    args.trimmed();
    args.advance();

    let text;

    if let Some(remains) = args.remains() {
        text = remains.to_owned();
        msg.delete(&ctx.http).await.context(here!())?;
    } else if let Some(src) = &msg.referenced_message {
        if src.author.bot {
            return Ok(None);
        }

        text = src.content_safe(&ctx.cache).await;
        msg.delete(&ctx.http).await.context(here!())?;
    } else {
        return Ok(None);
    }

    Ok(Some(text))
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
            0x0600..=0x06FF => "بيكو",
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
