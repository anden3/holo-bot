use std::time::Duration;

use anyhow::{anyhow, Context};
use backoff::{backoff::Backoff, ExponentialBackoff};
use chrono::{DateTime, Utc};
use chrono_tz::{Tz, UTC};
use futures::Future;
use reqwest::Response;
use serde::de::DeserializeOwned;
use tracing::{instrument, warn};
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

pub fn is_valid_timezone(timezone: &str) -> bool {
    timezone.parse::<Tz>().is_ok()
}

pub fn parse_written_time(time: &str, timezone: Option<&str>) -> anyhow::Result<DateTime<Utc>> {
    let local_timezone: Tz = timezone.and_then(|tz| tz.parse().ok()).unwrap_or(UTC);
    let local_time = Utc::now().with_timezone(&local_timezone);

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
