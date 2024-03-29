use std::str::FromStr;

use strum::IntoEnumIterator;
use strum_macros::{EnumIter, EnumString, ToString};

use utility::functions::parse_written_time;

use super::prelude::*;

#[derive(Debug, EnumString, EnumIter, ToString)]
pub enum TimestampFormat {
    Full,
    FullWeekday,
    Date,
    DateLong,
    Time,
    TimeSeconds,
    Relative,
}

impl TimestampFormat {
    pub fn description(&self) -> &'static str {
        match self {
            TimestampFormat::Full => "Date and time",
            TimestampFormat::FullWeekday => "Date and time, including day of the week",
            TimestampFormat::Date => "Date only",
            TimestampFormat::DateLong => "Date only, with month written out fully",
            TimestampFormat::Time => "Time only",
            TimestampFormat::TimeSeconds => "Time only, with seconds written out",
            TimestampFormat::Relative => "Relative time",
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

    fn get_modifier(&self) -> &'static str {
        match self {
            TimestampFormat::Full => "f",
            TimestampFormat::FullWeekday => "F",
            TimestampFormat::Date => "d",
            TimestampFormat::DateLong => "D",
            TimestampFormat::Time => "t",
            TimestampFormat::TimeSeconds => "T",
            TimestampFormat::Relative => "R",
        }
    }
}

interaction_setup! {
    name = "timestamp",
    group = "utility",
    description = "Given a relative time, outputs a Discord timestamp.",
    options = [
        //! What the time is.
        req when: String,
        //! Your timezone in IANA format (ex. America/New_York).
        timezone: String,
        //! The format of the timestamp.
        format: String = enum TimestampFormat,
    ]
}

#[interaction_cmd]
async fn timestamp(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    config: &Config,
) -> anyhow::Result<()> {
    parse_interaction_options!(
        interaction.data, [
        when: req String,
        timezone: String,
        format: enum TimestampFormat,
    ]);

    let time = match parse_written_time(&when, timezone.as_deref()) {
        Ok(time) => time,
        Err(e) => {
            interaction
                .create_interaction_response(&ctx, |r| {
                    r.kind(InteractionResponseType::ChannelMessageWithSource)
                        .interaction_response_data(|d| {
                            d.content(e.to_string())
                                .flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
                        })
                })
                .await?;
            return Err(e);
        }
    };

    let timestamp = time.timestamp();

    if let Some(format) = format {
        let timestamp_str = format.as_hint(timestamp);

        interaction
            .create_interaction_response(&ctx, |r| {
                r.kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|d| {
                        d.content(timestamp_str)
                            .flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
                    })
            })
            .await?;
    } else {
        interaction
            .create_interaction_response(&ctx, |r| {
                r.kind(InteractionResponseType::ChannelMessageWithSource)
                    .interaction_response_data(|d| {
                        d.create_embed(|e| {
                            e.fields(TimestampFormat::iter().map(|f| f.as_field(timestamp)))
                        })
                        .flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
                    })
            })
            .await?;
    }

    Ok(())
}
