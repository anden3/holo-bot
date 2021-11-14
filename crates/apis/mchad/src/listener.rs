use std::{fmt::Debug, pin::Pin, task::Poll, time::Duration};

use eventsource_client as es;
use futures::{Stream, TryStream};
use pin_project_lite::pin_project;
use tokio::sync::watch;
use tracing::{debug, error, trace, warn};

use crate::{
    types::{DataOrEmptyObject, EventData, Room, RoomEvent},
    util::validate_json_bytes,
};

pin_project! {
    pub struct Listener {
        pub room: watch::Receiver<Room>,
        #[pin]
        pub stream: es::EventStream<es::HttpsConnector>,
    }
}

impl std::fmt::Debug for Listener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Listener")
            .field("room", &self.room)
            .finish()
    }
}

impl Listener {
    pub(crate) fn new(room: watch::Receiver<Room>) -> miette::Result<Self> {
        let url = &format!(
            "https://repo.mchatx.org/Listener?room={}",
            &room.borrow().name
        );

        Ok(Self {
            room,
            stream: create_listener_stream(url)?,
        })
    }
}

impl Stream for Listener {
    type Item = EventData<RoomEvent>;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let this = self.project();
        let pinned_stream: Pin<&mut _> = this.stream;

        {
            let room = this.room.borrow();

            if !room.allows_external_sharing || room.needs_password {
                return Poll::Pending;
            }
        }

        poll_stream(pinned_stream, cx)
    }
}

pin_project! {
    pub struct EventListener<T> {
        #[pin]
        pub stream: es::EventStream<es::HttpsConnector>,
        event_type: std::marker::PhantomData<T>,
    }
}

impl<T> EventListener<T> {
    pub(crate) fn new(endpoint: &str) -> miette::Result<Self> {
        Ok(Self {
            stream: create_listener_stream(&format!(
                "https://repo.mchatx.org/PubSub/{}",
                endpoint
            ))?,
            event_type: std::marker::PhantomData,
        })
    }
}

impl<T> Stream for EventListener<T>
where
    T: Debug + for<'de> serde::Deserialize<'de>,
{
    type Item = T;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let this = self.project();
        let pinned_stream: Pin<&mut _> = this.stream;

        poll_stream(pinned_stream, cx)
    }
}

/* #[pin_project]
pub struct ArchiveListener {
    #[pin]
    pub stream: es::EventStream<es::HttpsConnector>,
}

impl ArchiveListener {
    pub(crate) fn new() -> anyhow::Result<Self> {
        Ok(Self {
            stream: create_listener_stream("https://repo.mchatx.org/PubSub/Archive")?,
        })
    }
}

impl Stream for ArchiveListener {
    type Item = ArchiveEvent;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let this = self.project();
        let pinned_stream: Pin<&mut _> = this.stream;

        poll_stream(pinned_stream, cx)
    }
}

#[pin_project]
pub struct RoomListener {
    #[pin]
    pub stream: es::EventStream<es::HttpsConnector>,
} */

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum RoomConfigUpdate {
    Locked,
    Unlocked,
    BlockedExternalSharing,
    UnblockedExternalSharing,
}

fn create_listener_stream(url: &str) -> miette::Result<es::EventStream<es::HttpsConnector>> {
    let client = es::Client::for_url(url)
        .map_err(|e| miette::miette!("{:?}", e))?
        .reconnect(
            es::ReconnectOptions::reconnect(true)
                .retry_initial(false)
                .delay(Duration::from_secs(1))
                .backoff_factor(2)
                .delay_max(Duration::from_secs(60))
                .build(),
        )
        .build();

    Ok(client.stream())
}

fn poll_stream<'a, T>(
    stream: Pin<&mut es::EventStream<es::HttpsConnector>>,
    cx: &mut std::task::Context<'a>,
) -> Poll<Option<T>>
where
    T: Debug,
    T: for<'de> serde::Deserialize<'de>,
{
    let next = stream.try_poll_next(cx);

    let event = match next {
        Poll::Ready(Some(Ok(e))) => e,
        Poll::Ready(Some(Err(e))) => {
            error!("{:?}", e);
            return Poll::Ready(None);
        }
        Poll::Ready(None) => return Poll::Ready(None),
        Poll::Pending => return Poll::Pending,
    };

    debug!(r#type = %event.event_type, "Event received!");

    let data = match event.field("data") {
        Some(data) => data,
        None => {
            warn!(?event, "Event didn't contain data.");
            return Poll::Ready(None);
        }
    };

    let data: T = match validate_json_bytes(data) {
        Ok(DataOrEmptyObject::Some(data)) => data,
        Ok(DataOrEmptyObject::None {}) => {
            return Poll::Pending;
        }
        Err(e) => {
            error!("{:?}", e);
            return Poll::Ready(None);
        }
    };

    trace!(?data, "Event data");
    Poll::Ready(Some(data))
}
