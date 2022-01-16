use regex::{Captures, Regex};

use utility::{
    functions::{try_get_timezone, try_parse_written_time_with_tz},
    regex_lazy,
};

use crate::commands::timestamp::TimestampFormat;

use super::prelude::*;

static TS_FMT_RGX: once_cell::sync::Lazy<Regex> = regex_lazy!(r"(?m)\{(.+?):?(\w)?\}");

#[poise::command(prefix_command, track_edits, required_permissions = "SEND_MESSAGES")]
/// Formats string and evaluates all time expressions enclosed in {..}.
pub(crate) async fn tsfmt(ctx: Context<'_>, #[rest] msg: String) -> anyhow::Result<()> {
    let mut args = Args::new(&msg, &[Delimiter::Single(' ')]);
    args.trimmed();

    let timezone = args.single::<String>()?;

    let timezone = match try_get_timezone(&timezone) {
        Ok(tz) => tz,
        Err(e) => {
            ctx.say(MessageBuilder::new().push_codeblock(e, None).build())
                .await?;

            return Ok(());
        }
    };

    let text = match args.remains() {
        Some(t) => t,
        None => return Ok(()),
    };

    let formatted_string = TS_FMT_RGX.replace_all(text, |caps: &Captures| {
        let time = match try_parse_written_time_with_tz(&caps[1], timezone) {
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

    ctx.say(
        MessageBuilder::new()
            .push_codeblock(formatted_string.clone(), None)
            .push(formatted_string)
            .build(),
    )
    .await?;

    Ok(())
}
