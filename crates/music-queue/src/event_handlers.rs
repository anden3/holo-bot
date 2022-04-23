use super::{events::QueueUpdate, prelude::*};

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

pub(crate) struct SendEvent {
    pub channel: mpsc::Sender<QueueUpdate>,
    pub event: QueueUpdate,
}

#[async_trait]
impl EventHandler for SendEvent {
    #[instrument(skip(self, _ctx))]
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        if let Err(e) = self.channel.send(self.event.clone()).await {
            error!("{:?}", e);
        }

        None
    }
}

pub(crate) struct GlobalEvent {
    pub channel: mpsc::Sender<QueueUpdate>,
}

#[async_trait]
impl EventHandler for GlobalEvent {
    #[instrument(skip(self, ctx))]
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        let update = match ctx {
            EventContext::ClientDisconnect(client) => {
                QueueUpdate::ClientDisconnected(UserId(client.user_id.0))
            }
            _ => {
                error!(event = ?ctx, "Unhandled event!");
                return None;
            }
        };

        if let Err(e) = self.channel.send(update).await {
            error!("{:?}", e);
        }

        None
    }
}
