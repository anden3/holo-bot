use anyhow::Context;
use reqwest::Response;
use serde::de::DeserializeOwned;
use tracing::error;

#[macro_use]
extern crate fix_hidden_lifetime_bug;

pub mod config;
pub mod discord;
pub mod extensions;
pub mod logger;
pub mod macros;
pub mod serializers;
pub mod streams;

pub async fn validate_response<T>(response: Response) -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    if let Err(error_code) = (&response).error_for_status_ref().context(here!()) {
        let response_bytes = response.bytes().await.context(here!())?;
        let deserializer = &mut serde_json::Deserializer::from_slice(&response_bytes);
        let response: Result<T, _> = serde_path_to_error::deserialize(deserializer);

        match response {
            Ok(_) => Err(error_code),
            Err(e) => {
                error!(
                    "Deserialization error at '{}' in {}.",
                    e.path().to_string(),
                    here!()
                );
                error!(
                    "Data:\r\n{:?}",
                    std::str::from_utf8(&response_bytes).context(here!())?
                );
                Err(e.into())
            }
        }
    } else {
        let response_bytes = response.bytes().await.context(here!())?;
        let deserializer = &mut serde_json::Deserializer::from_slice(&response_bytes);
        let response: Result<T, _> = serde_path_to_error::deserialize(deserializer);

        match response {
            Ok(response) => Ok(response),
            Err(e) => {
                error!(
                    "Deserialization error at '{}' in {}.",
                    e.path().to_string(),
                    here!()
                );
                error!(
                    "Data:\r\n{:?}",
                    std::str::from_utf8(&response_bytes).context(here!())?
                );
                Err(e.into())
            }
        }
    }
}
