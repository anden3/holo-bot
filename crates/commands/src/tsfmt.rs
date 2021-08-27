use regex::{Captures, Regex};

use utility::{
    functions::{is_valid_timezone, parse_written_time},
    regex_lazy,
};

use super::prelude::*;
use crate::timestamp::TimestampFormat;

static TS_FMT_RGX: once_cell::sync::Lazy<Regex> = regex_lazy!(r"(?m)\{(.+?):?(\w)?\}");

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
/// Formats string and evaluates all time expressions enclosed in {..}.
pub async fn tsfmt(ctx: &Ctx, msg: &Message) -> CommandResult {
    let mut args = Args::new(
        &msg.content_safe(&ctx.cache).await,
        &[Delimiter::Single(' ')],
    );
    args.trimmed();
    args.advance();

    let timezone = if is_valid_timezone(&args.parse::<String>()?) {
        Some(args.single::<String>()?)
    } else {
        None
    };

    let text = match args.remains() {
        Some(t) => t,
        None => return Ok(()),
    };

    let formatted_string = TS_FMT_RGX.replace_all(text, |caps: &Captures| {
        let time = match parse_written_time(&caps[1], timezone.as_deref()) {
            Ok(time) => time,
            Err(_) => return "INVALID FORMAT".to_string(),
        };

        let fmt = if let Some(format) = caps.get(2) {
            TimestampFormat::from_modifier(format.as_str()).unwrap_or(TimestampFormat::Full)
        } else {
            TimestampFormat::Full
        };

        fmt.parse_timestamp(time.timestamp())
    });

    msg.reply(
        &ctx.http,
        MessageBuilder::new()
            .push_codeblock(formatted_string, None)
            .build(),
    )
    .await?;

    Ok(())
}
