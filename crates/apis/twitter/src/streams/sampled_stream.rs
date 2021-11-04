use futures::Stream;
use tokio::sync::mpsc;

use crate::{errors::Error, streams::twitter_stream::TwitterStream, types::*};

pub struct SampledStream {
    tweet_stream: mpsc::Receiver<Tweet>,
    exit_notifier: mpsc::Sender<()>,
}

impl SampledStream {
    pub async fn new(token: &str, parameters: StreamParameters) -> Result<Self, Error> {
        Self::with_buffer_size(token, parameters, 64).await
    }

    pub async fn with_buffer_size(
        token: &str,
        parameters: StreamParameters,
        buffer_size: usize,
    ) -> Result<Self, Error> {
        let client = TwitterStream::initialize_client(token)?;

        let (tweet_stream, exit_notifier) =
            TwitterStream::create("/2/tweets/sample/stream", client, parameters, buffer_size)
                .await?;

        Ok(Self {
            tweet_stream,
            exit_notifier,
        })
    }
}

impl Stream for SampledStream {
    type Item = Tweet;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.tweet_stream.poll_recv(cx)
    }
}

impl AsRef<mpsc::Receiver<Tweet>> for SampledStream {
    fn as_ref(&self) -> &mpsc::Receiver<Tweet> {
        &self.tweet_stream
    }
}

impl AsMut<mpsc::Receiver<Tweet>> for SampledStream {
    fn as_mut(&mut self) -> &mut mpsc::Receiver<Tweet> {
        &mut self.tweet_stream
    }
}

impl Drop for SampledStream {
    fn drop(&mut self) {
        let _ = self.exit_notifier.send(());
    }
}
