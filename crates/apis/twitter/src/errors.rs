//! Types for various errors that can occur when interacting with the API.
#![allow(clippy::enum_variant_names)]

use chrono::{DateTime, Utc};
use thiserror::Error;

use crate::{ApiError, Rule, RuleId};

#[derive(Error, Debug)]
/// Errors that can occur when interacting with the Holodex API.
pub enum Error {
    #[error("API token contains invalid characters.")]
    /// The API token provided to the client is invalid.
    InvalidApiToken,
    #[error("Error creating HTTP client: {0:?}")]
    /// An error occurred while creating the HTTP client.
    HttpClientCreationError(#[source] hyper::Error),
    #[error("Error sending request to {endpoint}: {source:?}")]
    /// An error occurred while sending an API request.
    ApiRequestFailed {
        /// The endpoint that was queried.
        endpoint: &'static str,
        #[source]
        /// The error that was encountered.
        source: hyper::Error,
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
    #[error("Rate limit reached.")]
    /// Too many requests have been made to the API.
    RateLimitReached {
        /// How many requests have been made within the time window.
        requests_made: i32,
        /// How many requests are allowed to be made within the time window.
        request_limit: i32,
        /// When the rate limit resets.
        resets_at: DateTime<Utc>,
    },
    #[error("Too many rules were added to a filtered stream: ({count}/{limit})")]
    /// Too many rules were added to a filtered stream.
    RuleLimitExceeded {
        /// The number of rules that were provided.
        count: usize,
        /// The maximum number of rules.
        limit: usize,
    },
    #[error("A rule added to a filtered stream was too long: ({length}/{limit})")]
    /// A rule added to a filtered stream was too long.
    RuleLengthExceeded {
        /// The rule that was provided.
        rule: String,
        /// The length of the rule.
        length: usize,
        /// The maximum rule length.
        limit: usize,
    },
    #[error("{count} invalid rules found! Rules are {rules:?}.")]
    /// Invalid rules were provided.
    InvalidRules {
        /// The number of invalid rules.
        count: usize,
        /// The rules that were provided.
        rules: Vec<Rule>,
    },
    #[error("Failed to delete {failed_deletion_count} rules: {rules_to_be_deleted:?}")]
    /// Failed to delete rules.
    RuleDeletionFailed {
        failed_deletion_count: usize,
        rules_to_be_deleted: Vec<RuleId>,
    },
    #[error("Missing response header: {0}")]
    /// A header is missing from the response.
    MissingResponseHeader(&'static str),
    #[error("Invalid response header: {0}")]
    /// A response header could not be parsed.
    InvalidResponseHeader(&'static str),
    #[error("Twitter API errors: {0:?}")]
    ApiErrors(Vec<ApiError>),
    #[error("A command sent to the underlying stream failed.")]
    StreamCommandFailed(Box<dyn std::error::Error + Send + Sync>),
}

#[derive(Error, Debug)]
/// Errors that can occur when validating a response from the Twitter API.
pub enum ValidationError {
    #[error("Server error: {0:#?}")]
    /// The API returned a server error.
    ServerError(#[from] ServerError),
    #[error("Parse error: {0:#?}")]
    /// The response from the API could not be parsed.
    ParseError(#[from] ParseError),
}

#[derive(Error, Debug)]
/// Errors that occur when the API returns an error code.
pub enum ServerError {
    #[error("Server returned an error code: {0}")]
    /// The API returned an error code.
    ErrorCode(hyper::StatusCode),
    #[error("Server returned error {0} with message: {1}")]
    /// The API returned an error code with a message.
    ErrorCodeWithValue(hyper::StatusCode, String),
    #[error("Server returned error {0} with a message that could not be parsed: {1:?}")]
    /// The API returned an error code with a message that could not be parsed.
    ErrorCodeWithValueParseError(hyper::StatusCode, ParseError),
}

#[derive(Error, Debug)]
/// Errors that occur when parsing a response from the API.
pub enum ParseError {
    #[error("Could not decode response: {0:?}")]
    /// The response from the API could not be converted into bytes.
    ResponseDecodeError(#[source] hyper::Error),
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
