use super::{events::QueueUpdate, prelude::*};

pub(crate) struct UpdateBufferAfterSongEnded {
    channel: mpsc::Sender<QueueUpdate>,
}

#[async_trait]
impl EventHandler for UpdateBufferAfterSongEnded {
    #[instrument(skip(self, _ctx))]
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        if let Err(e) = self.channel.send(QueueUpdate::TrackEnded).await {
            error!("{:?}", e);
        }

        None
    }
}
