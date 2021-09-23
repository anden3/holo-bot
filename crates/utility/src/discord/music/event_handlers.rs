use super::{prelude::*, queue_events::QueueUpdate};

pub(crate) struct UpdateBufferAfterSongEnded {
    pub channel: mpsc::Sender<QueueUpdate>,
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
