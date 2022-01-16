use utility::functions::try_parse_written_time;

use super::prelude::*;

#[derive(Debug, SlashChoiceParameter)]
pub enum TimestampFormat {
    #[name = "Full"]
    Full,
    #[name = "Full with weekday"]
    FullWeekday,
    #[name = "Date only"]
    Date,
    #[name = "Only the date, in long format"]
    DateLong,
    #[name = "Time only"]
    Time,
    #[name = "Time only, with seconds"]
    TimeSeconds,
    #[name = "Relative"]
    Relative,
}

impl TimestampFormat {
    pub fn description(&self) -> &'static str {
        match self {
            Self::Full => "Date and time",
            Self::FullWeekday => "Date and time, including day of the week",
            Self::Date => "Date only",
            Self::DateLong => "Date only, with month written out fully",
            Self::Time => "Time only",
            Self::TimeSeconds => "Time only, with seconds written out",
            Self::Relative => "Relative time",
        }
    }

    pub fn parse_timestamp(&self, timestamp: i64) -> String {
        format!("<t:{}:{}>", timestamp, self.get_modifier())
    }

    pub fn as_hint(&self, timestamp: i64) -> String {
        format!("`{0}` => {0}", self.parse_timestamp(timestamp))
    }

    pub fn as_field(&self, timestamp: i64) -> (String, String, bool) {
        (
            self.description().to_string(),
            self.as_hint(timestamp),
            false,
        )
    }

    fn get_modifier(&self) -> &'static str {
        match self {
            Self::Full => "f",
            Self::FullWeekday => "F",
            Self::Date => "d",
            Self::DateLong => "D",
            Self::Time => "t",
            Self::TimeSeconds => "T",
            Self::Relative => "R",
        }
    }

    fn fields(timestamp: i64) -> impl Iterator<Item = (String, String, bool)> {
        [
            Self::Full,
            Self::FullWeekday,
            Self::Date,
            Self::DateLong,
            Self::Time,
            Self::TimeSeconds,
            Self::Relative,
        ]
        .into_iter()
        .map(move |f| f.as_field(timestamp))
    }

    pub fn from_modifier(modifier: &str) -> Option<Self> {
        match modifier {
            "f" => Some(TimestampFormat::Full),
            "F" => Some(TimestampFormat::FullWeekday),
            "d" => Some(TimestampFormat::Date),
            "D" => Some(TimestampFormat::DateLong),
            "t" => Some(TimestampFormat::Time),
            "T" => Some(TimestampFormat::TimeSeconds),
            "R" => Some(TimestampFormat::Relative),
            _ => None,
        }
    }
}

#[poise::command(
    slash_command,
    prefix_command,
    track_edits,
    required_permissions = "SEND_MESSAGES"
)]
/// Given a relative time, outputs a Discord timestamp.
pub(crate) async fn timestamp(
    ctx: Context<'_>,

    #[description = "What the time is"] when: String,
    #[description = "Your timezone in IANA format (ex. America/New_York)."] timezone: Option<
        String,
    >,
    #[description = "The format of the timestamp."] format: Option<TimestampFormat>,
) -> anyhow::Result<()> {
    ctx.defer_ephemeral().await?;

    let time = match try_parse_written_time(&when, timezone.as_deref()) {
        Ok(time) => time,
        Err(e) => {
            ctx.say(MessageBuilder::new().push_codeblock(e, None).build())
                .await?;

            return Ok(());
        }
    };

    let timestamp = time.timestamp();

    if let Some(format) = format {
        let timestamp_str = format.as_hint(timestamp);

        ctx.say(timestamp_str).await?;
    } else {
        ctx.send(|m| {
            m.embed(|e| {
                e.title(format!("{when} in {}", time.timezone()))
                    .fields(TimestampFormat::fields(timestamp))
            })
        })
        .await?;
    }

    Ok(())
}
