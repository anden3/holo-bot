//! Types for various errors that can occur when interacting with the API.

use miette::Diagnostic;
use thiserror::Error;

#[derive(Error, Diagnostic, Debug)]
/// Errors that can occur when interacting with the Holodex API.
pub enum Error {
    #[error("API token contains invalid characters.")]
    /// The API token provided to the client is invalid.
    InvalidApiToken,
    #[error("Error creating HTTP client: {0:?}")]
    /// An error occurred while creating the HTTP client.
    HttpClientCreationError(#[source] reqwest::Error),
    #[error("Error sending request to {endpoint}: {source:?}")]
    /// An error occurred while sending an API request.
    ApiRequestFailed {
        /// The endpoint that was queried.
        endpoint: &'static str,
        #[source]
        /// The error that was encountered.
        source: reqwest::Error,
    },
    #[error("Invalid response received from endpoint ({endpoint}).")]
    /// The API returned a faulty response or server error.
    InvalidResponse {
        /// The endpoint that was queried.
        endpoint: &'static str,
        #[source]
        /// The error that was encountered.
        source: ValidationError,
    },
    #[error("The provided video ID was not valid: {0}")]
    /// An invalid video ID was passed to the API.
    InvalidVideoId(String),

    #[error("The provided channel ID was not valid: {0}")]
    /// An invalid channel ID was passed to the API.
    InvalidChannelId(String),

    #[error("The filter could not be constructed due to invalid arguments.")]
    /// A filter could not be constructed due to invalid arguments.
    FilterCreationError(String),
}

#[derive(Error, Diagnostic, Debug)]
/// Errors that can occur when validating a response from the Holodex API.
pub enum ValidationError {
    #[error("Server error: {0:?}")]
    /// The API returned a server error.
    ServerError(#[from] ServerError),
    #[error("Parse error: {0:?}")]
    /// The response from the API could not be parsed.
    ParseError(#[from] ParseError),
}

#[derive(Error, Diagnostic, Debug)]
/// Errors that occur when the API returns an error code.
pub enum ServerError {
    #[error("Server returned an error code: {0}")]
    /// The API returned an error code.
    ErrorCode(#[from] reqwest::Error),
    #[error("Server returned error {0} with message: {1}")]
    /// The API returned an error code with a message.
    ErrorCodeWithValue(#[source] reqwest::Error, String),
    #[error("Server returned error {0} with a message that could not be parsed: {1:?}")]
    /// The API returned an error code with a message that could not be parsed.
    ErrorCodeWithValueParseError(#[source] reqwest::Error, ParseError),
}

#[derive(Error, Diagnostic, Debug)]
/// Errors that occur when parsing a response from the API.
pub enum ParseError {
    #[error("Could not decode response: {0:?}")]
    /// The response from the API could not be converted into bytes.
    ResponseDecodeError(#[source] reqwest::Error),
    #[error("Failed to parse response as JSON: {0:?}\nResponse: {1}")]
    /// The response from the API could not be parsed as JSON.
    ResponseJsonParseError(#[source] serde_json::Error, String),
    #[error("Failed to parse response: {0:?}\nResponse: {1}")]
    /// The response from the API could not be parsed.
    ResponseParseError(#[source] serde_json::Error, serde_json::Value),
    #[error("Response was neither valid JSON nor valid UTF-8.")]
    /// The response from the API could not be parsed as JSON or UTF-8.
    ResponseUtf8Error(#[from] std::str::Utf8Error),
}
