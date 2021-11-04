use std::str::FromStr;

use backoff::backoff::Backoff;
use chrono::{DateTime, NaiveDateTime, Utc};
use futures::Future;
use reqwest::Response;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::errors::{Error, ParseError, ServerError, ValidationError};

pub async fn validate_response<T>(response: reqwest::Response) -> Result<T, ValidationError>
where
    T: for<'de> Deserialize<'de> + std::fmt::Debug,
{
    if let Err(error_code) = (&response).error_for_status_ref() {
        let bytes = match response.bytes().await {
            Ok(b) => b,
            Err(e) => {
                return Err(ServerError::ErrorCodeWithValueParseError(
                    error_code,
                    ParseError::ResponseDecodeError(e),
                )
                .into())
            }
        };

        Err(match validate_json_bytes::<T>(&bytes) {
            Ok(val) => ServerError::ErrorCodeWithValue(error_code, format!("{:?}", val)).into(),
            Err(error) => ServerError::ErrorCodeWithValueParseError(error_code, error).into(),
        })
    } else {
        let bytes = response
            .bytes()
            .await
            .map_err(|e| ValidationError::ParseError(ParseError::ResponseDecodeError(e)))?;

        validate_json_bytes(&bytes).map_err(|e| e.into())
    }
}

pub fn validate_json_bytes<T>(bytes: &[u8]) -> Result<T, ParseError>
where
    T: for<'de> Deserialize<'de> + std::fmt::Debug,
{
    let data: Result<T, _> = serde_json::from_slice(bytes);

    match data {
        Ok(data) => Ok(data),
        Err(e) => Err(match serde_json::from_slice::<serde_json::Value>(bytes) {
            Ok(v) => ParseError::ResponseParseError(e, v),
            Err(e) => match std::str::from_utf8(bytes) {
                Ok(s) => ParseError::ResponseJsonParseError(e, s.to_owned()),
                Err(e) => ParseError::ResponseUtf8Error(e),
            },
        }),
    }
}

pub async fn try_run_with_config<F, R, E, C, Fut>(func: F, config: C) -> Result<R, E>
where
    F: Fn() -> Fut,
    E: std::error::Error,
    C: Backoff,
    Fut: Future<Output = Result<R, E>>,
{
    Ok(backoff::future::retry(config, || async {
        let streams = func().await.map_err(|e| {
            warn!("{:?}", e);
            e
        })?;

        Ok(streams)
    })
    .await?)
}

pub(crate) fn check_rate_limit(response: &Response) -> Result<(), Error> {
    let remaining: i32 = get_response_header("x-rate-limit-remaining", response)?;
    let limit = get_response_header("x-rate-limit-limit", response)?;
    let reset = get_response_header("x-rate-limit-reset", response)?;

    // Convert timestamp to local time.
    let reset = NaiveDateTime::from_timestamp(reset, 0);
    let reset: DateTime<Utc> = DateTime::from_utc(reset, Utc);

    // Get duration until reset happens.
    let time = Utc::now() - reset;

    debug!(
        "{}/{} requests made (Resets in {:02}:{:02}:{:02})",
        limit - remaining,
        limit,
        time.num_hours(),
        time.num_minutes() % 60,
        time.num_seconds() % 60
    );

    if remaining <= 0 {
        Err(Error::RateLimitReached {
            requests_made: limit - remaining,
            request_limit: limit,
            resets_at: reset,
        })
    } else {
        Ok(())
    }
}

pub(crate) fn get_response_header<T: FromStr>(
    header: &'static str,
    response: &Response,
) -> Result<T, Error> {
    response
        .headers()
        .get(header)
        .ok_or(Error::MissingResponseHeader(header))?
        .to_str()
        .map_err(|_| Error::InvalidResponseHeader(header))?
        .parse::<T>()
        .map_err(|_| Error::InvalidResponseHeader(header))
}

#[derive(Serialize, Deserialize)]
pub struct DurationMinutes(i64);

impl From<DurationMinutes> for chrono::Duration {
    fn from(m: DurationMinutes) -> Self {
        Self::minutes(m.0)
    }
}

pub trait VecExt<T> {
    fn sort_unstable_by_key_ref<F, K>(&mut self, key: F)
    where
        F: Fn(&T) -> &K,
        K: ?Sized + Ord;
}

impl<T> VecExt<T> for Vec<T> {
    fn sort_unstable_by_key_ref<F, K>(&mut self, key: F)
    where
        F: Fn(&T) -> &K,
        K: ?Sized + Ord,
    {
        self.sort_unstable_by(|x, y| key(x).cmp(key(y)));
    }
}

pub fn is_default<T: Default + PartialEq>(t: &T) -> bool {
    t == &T::default()
}
