use std::io::Read;

use serde::Deserialize;

use crate::errors::{ParseError, ServerError, ValidationError};

fn into_bytes(response: ureq::Response) -> Result<Vec<u8>, ParseError> {
    let mut buffer = match response
        .header("Content-Length")
        .and_then(|s| s.parse::<usize>().ok())
    {
        Some(len) => Vec::with_capacity(len),
        None => Vec::new(),
    };

    match response.into_reader().read_to_end(&mut buffer) {
        Ok(_) => Ok(buffer),
        Err(e) => Err(ParseError::ResponseDecodeError(e)),
    }
}

pub fn validate_response<T>(
    response: Result<ureq::Response, ureq::Error>,
) -> Result<T, ValidationError>
where
    T: for<'de> Deserialize<'de> + std::fmt::Debug,
{
    match response {
        Ok(response) => {
            let bytes = into_bytes(response).map_err(ValidationError::ParseError)?;
            validate_json_bytes(&bytes).map_err(|e| e.into())
        }
        Err(ureq::Error::Status(status, response)) => {
            let bytes = into_bytes(response).map_err(|e| {
                ValidationError::ServerError(ServerError::ErrorCodeWithValueParseError(status, e))
            })?;

            Err(match validate_json_bytes::<T>(&bytes) {
                Ok(val) => ServerError::ErrorCodeWithValue(status, format!("{:?}", val)).into(),
                Err(error) => ServerError::ErrorCodeWithValueParseError(status, error).into(),
            })
        }
        Err(e @ ureq::Error::Transport(_)) => {
            Err(ValidationError::ServerError(ServerError::TransportError(e)))
        }
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

/* pub async fn try_run<F, R, Fut>(func: F) -> anyhow::Result<R>
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
            warn!("{:?}", e);
            anyhow!(e).context(here!())
        })?;

        Ok(streams)
    })
    .await
    .context(here!())?)
} */
