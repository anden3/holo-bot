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
        r#"(?msx)                                                           # Flags
        (?P<text>.*?[\w&&[^_]]+.*?)                                         # Text, not including underscores at the end.
        (?P<punct>
            [\.!\?\u3002\uFE12\uFE52\uFF0E\uFF61\uFF01\uFF1F"_\*`\)]+       # Match punctuation not at the end of a line.
            |
            \s*(?:                                                          # Include eventual whitespace after peko.
                [\.!\?\u3002\uFE12\uFE52\uFF0E\uFF61\uFF01\uFF1F"_\*`\)]    # Match punctuation at the end of a line.
                |
                (?:<:\w+:\d+>)                                              # Match Discord emotes at the end of a line.
                |
                [\x{1F600}-\x{1F64F}]                                       # Match Unicode emoji at the end of a line.
            )*$
        )"#
    );

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
            return Ok(());
        }

        text = src.content_safe(&ctx.cache).await;
        msg.delete(&ctx.http).await.context(here!())?;
    } else {
        return Ok(());
    }

    if text.starts_with("-pekofy") {
        msg.channel_id
            .say(&ctx.http, "Nice try peko")
            .await
            .context(here!())?;
        return Ok(());
    }

    let mut pekofied_text = String::with_capacity(text.len());

    for capture in sentence_rgx.captures_iter(&text) {
        if capture.get(0).unwrap().as_str().trim().is_empty() {
            continue;
        }

        let text = capture
            .name("text")
            .ok_or_else(|| anyhow!("Couldn't find 'text' capture!"))
            .context(here!())?
            .as_str();

        let text_is_uppercase = text == text.to_uppercase();

        // Get response based on alphabet used.
        let response = match text
            .chars()
            .last()
            .ok_or_else(|| anyhow!("Can't get last character!"))
            .context(here!())? as u32
        {
            0x0400..=0x04FF => {
                if text_is_uppercase {
                    " ПЕКО"
                } else {
                    " пеко"
                }
            }
            0x3040..=0x30FF | 0xFF00..=0xFFEF | 0x4E00..=0x9FAF => "ぺこ",
            _ => {
                if text_is_uppercase {
                    " PEKO"
                } else {
                    " peko"
                }
            }
        };

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
