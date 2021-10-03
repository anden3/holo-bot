use std::{borrow::Cow, collections::HashMap, time::Duration};

use anyhow::{anyhow, Context};
use backoff::{backoff::Backoff, ExponentialBackoff};
use chrono::{DateTime, Utc};
use chrono_tz::{Tz, TZ_VARIANTS, UTC};
use futures::Future;
use once_cell::sync::Lazy;
use reqwest::Response;
use serde::de::DeserializeOwned;
use str_utils::StartsWithIgnoreCase;
use tracing::{instrument, warn};
use unicase::Ascii as UniCase;
use unicode_truncate::UnicodeTruncateStr;

use crate::here;

#[instrument]
pub async fn validate_response<T>(response: Response) -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    if let Err(error_code) = (&response).error_for_status_ref().context(here!()) {
        eprintln!("Request gave error code: {:?}", error_code);
        validate_json_bytes::<T>(&response.bytes().await.context(here!())?).or(Err(error_code))
    } else {
        validate_json_bytes(&response.bytes().await.context(here!())?)
    }
}

#[instrument(skip(bytes))]
pub fn validate_json_bytes<T>(bytes: &[u8]) -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    let deserializer = &mut serde_json::Deserializer::from_slice(bytes);
    let data: Result<T, _> = serde_path_to_error::deserialize(deserializer);

    match data {
        Ok(data) => Ok(data),
        Err(e) => {
            eprintln!(
                "Deserialization error at '{}' in {}.",
                e.path().to_string(),
                here!()
            );

            match serde_json::from_slice::<serde_json::Value>(bytes) {
                Ok(v) => {
                    let mut data = format!("{}", v);

                    if data.len() >= 1024 {
                        let (truncated_data, _len) = data.unicode_truncate(1024);
                        data = truncated_data.to_string();
                    }

                    eprintln!("Data:\r\n{}", data);
                }
                Err(e) => {
                    eprintln!("Failed to convert data to JSON: {:?}", e);
                    eprintln!(
                        "Data:\r\n{:?}",
                        std::str::from_utf8(bytes).context(here!())?
                    );
                }
            }

            Err(e.into())
        }
    }
}

pub async fn try_run<F, R, Fut>(func: F) -> anyhow::Result<R>
where
    F: Fn() -> Fut,
    Fut: Future<Output = anyhow::Result<R>>,
{
    try_run_with_config(
        func,
        ExponentialBackoff {
            initial_interval: Duration::from_secs(4),
            max_interval: Duration::from_secs(64 * 60),
            randomization_factor: 0.0,
            multiplier: 2.0,
            ..ExponentialBackoff::default()
        },
    )
    .await
}

pub async fn try_run_with_config<F, R, C, Fut>(func: F, config: C) -> anyhow::Result<R>
where
    F: Fn() -> Fut,
    C: Backoff,
    Fut: Future<Output = anyhow::Result<R>>,
{
    Ok(backoff::future::retry(config, || async {
        let streams = func().await.map_err(|e| {
            warn!("{}", e.to_string());
            anyhow!(e).context(here!())
        })?;

        Ok(streams)
    })
    .await
    .context(here!())?)
}

#[allow(clippy::type_complexity)]
static TIMEZONE_PARTS: Lazy<HashMap<[Option<UniCase<Cow<str>>>; 3], Tz>> = Lazy::new(|| {
    TZ_VARIANTS
        .iter()
        .map(|t| {
            let arr = match t.name().split('/').collect::<Vec<_>>().as_slice() {
                [a] => [Some(UniCase::new(Cow::Borrowed(*a))), None, None],
                [a, b] => [
                    Some(UniCase::new(Cow::Borrowed(*a))),
                    Some(UniCase::new(Cow::Borrowed(*b))),
                    None,
                ],
                [a, b, c] => [
                    Some(UniCase::new(Cow::Borrowed(*a))),
                    Some(UniCase::new(Cow::Borrowed(*b))),
                    Some(UniCase::new(Cow::Borrowed(*c))),
                ],
                [] | [_, _, _, ..] => {
                    panic!("Invalid timezone name: {}", t.name());
                }
            };

            (arr, t.to_owned())
        })
        .collect()
});

pub fn is_valid_timezone(timezone: &str) -> bool {
    try_get_timezone(timezone).is_ok()
}

pub fn try_get_timezone(timezone: &str) -> anyhow::Result<&Tz> {
    if !timezone.is_ascii() {
        return Err(anyhow!("Non-ASCII characters in timezone: {}", timezone));
    }

    let parts = timezone
        .split('/')
        .map(|p| UniCase::new(Cow::Borrowed(p)))
        .collect::<Vec<_>>();

    let part_count = parts.len();

    if part_count > 3 || part_count == 0 {
        return Err(anyhow!("Invalid timezone: {}", timezone));
    }

    let mut parts = parts.into_iter().fuse();
    let key = [parts.next(), parts.next(), parts.next()];

    // Fast path, no auto-complete necessary.
    if let Some(tz) = TIMEZONE_PARTS.get(&key) {
        return Ok(tz);
    }

    let partial_matches = TIMEZONE_PARTS
        .keys()
        .filter(|m| m.iter().filter(|k| k.is_some()).count() == part_count)
        .filter(|m| {
            for i in 0..part_count {
                if m[i].as_ref().unwrap() != key[i].as_ref().unwrap()
                    && !m[i]
                        .as_ref()
                        .unwrap()
                        .starts_with_ignore_case(&***key[i].as_ref().unwrap())
                {
                    return false;
                }
            }

            true
        })
        .collect::<Vec<_>>();

    match partial_matches.len() {
        0 => Err(anyhow!("No matching timezones found for {}", timezone)),
        1 => Ok(TIMEZONE_PARTS.get(partial_matches[0]).unwrap()),
        n => {
            let mut timezone_names = partial_matches
                .into_iter()
                .map(|m| format!("\t{}", TIMEZONE_PARTS.get(m).unwrap().name()))
                .collect::<Vec<_>>();

            timezone_names.sort();

            Err(anyhow!(
                "{} timezones matched '{}':\n{}",
                n,
                &timezone,
                timezone_names.join("\n")
            ))
        }
    }
}

pub fn try_parse_written_time(time: &str, timezone: Option<&str>) -> anyhow::Result<DateTime<Utc>> {
    let local_timezone = match timezone {
        Some(tz) => try_get_timezone(tz)?,
        None => &UTC,
    };

    try_parse_written_time_with_tz(time, local_timezone)
}

pub fn try_parse_written_time_with_tz(time: &str, timezone: &Tz) -> anyhow::Result<DateTime<Utc>> {
    let local_time = Utc::now().with_timezone(timezone);

    let time = {
        if let Some(s) = time.strip_prefix("in ") {
            s
        } else if let Some(s) = time.strip_prefix("at ") {
            s
        } else {
            time
        }
    };

    let time = chrono_english::parse_date_string(time, local_time, chrono_english::Dialect::Us)
        .context(here!())?;

    Ok(time.with_timezone(&Utc))
}

pub fn is_default<T: Default + PartialEq>(t: &T) -> bool {
    t == &T::default()
}

pub fn default_true() -> bool {
    true
}
