use songbird::tracks::TrackError;
use thiserror::Error;
use tokio::sync::mpsc::error::SendError;

use crate::events::QueueUpdate;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Operation failed: {0}")]
    OperationFailed(String),
    #[error("Track operation failed: {0:?}")]
    TrackOperationFailed(#[from] TrackError),
    #[error("Failed to send queue update message: {0:?}")]
    UpdateSendingFailed(#[from] SendError<QueueUpdate>),
    #[error("Failed to parse ID: {0:?}")]
    IdParsingFailed(#[from] ytextract::error::Id<0>),
    #[error("Extraction error: {0:?}")]
    ExtractionFailed(#[from] ytextract::error::Error),
}
