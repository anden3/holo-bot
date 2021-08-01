use std::{collections::HashMap, pin::Pin, sync::Arc, task::Poll, time::Duration};

use anyhow::Context;
use futures::{Stream, TryStream};
use pin_project::pin_project;
use reqwest::Client;
use tokio::{
    sync::{broadcast, watch, Mutex},
    time::sleep,
};
use tracing::{debug, error, instrument, trace, warn};

use utility::{
    async_clone,
    functions::{try_run, validate_json_bytes, validate_response},
    here, regex,
};

use crate::types::mchad_api::*;

use eventsource_client as es;

#[derive(Debug)]
pub struct MchadApi {
    pub room_updates: broadcast::Receiver<RoomUpdate>,
    client: Client,
    rooms: Arc<Mutex<HashMap<String, Room>>>,
    listeners: Arc<Mutex<HashMap<String, watch::Sender<Room>>>>,
}

impl MchadApi {
    const USER_AGENT: &'static str =
        concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);
    const ROOM_UPDATE_INTERVAL: Duration = Duration::from_secs(60);

    pub fn connect() -> Self {
        let client = reqwest::ClientBuilder::new()
            .user_agent(Self::USER_AGENT)
            .build()
            .context(here!())
            .unwrap();

        let rooms = Arc::new(Mutex::new(HashMap::new()));
        let listeners = Arc::new(Mutex::new(HashMap::new()));
        let (room_update_tx, room_update_rx) = broadcast::channel(16);

        tokio::spawn(async_clone!(client, rooms, listeners; {
            if let Err(e) = Self::updater(client, rooms, listeners, room_update_tx).await {
                error!("Error: {}", e);
            }
        }));

        Self {
            client,
            rooms,
            listeners,
            room_updates: room_update_rx,
        }
    }

    #[instrument(skip(self))]
    pub async fn get_listener(&mut self, stream: &str) -> Option<Listener> {
        let rooms = self.rooms.lock().await;
        let room = rooms
            .iter()
            .find(|(_, r)| r.stream == Some(stream.to_string()))?
            .1;

        let (listener_tx, listener_rx) = watch::channel(room.clone());

        match Listener::new(listener_rx) {
            Ok(listener) => {
                self.listeners
                    .lock()
                    .await
                    .insert(room.name.clone(), listener_tx);

                Some(listener)
            }
            Err(e) => {
                error!("{:?}", e);
                None
            }
        }
    }

    #[instrument(skip(client, rooms, room_update_sender))]
    async fn updater(
        client: Client,
        rooms: Arc<Mutex<HashMap<String, Room>>>,
        listeners: Arc<Mutex<HashMap<String, watch::Sender<Room>>>>,
        room_update_sender: broadcast::Sender<RoomUpdate>,
    ) -> anyhow::Result<()> {
        loop {
            let res = try_run(|| async {
                client
                    .get("https://repo.mchatx.org/Room")
                    .send()
                    .await
                    .context(here!())
            })
            .await?;

            let new_rooms: Vec<Room> = match validate_response(res).await.context(here!()) {
                Ok(val) => val,
                Err(e) => {
                    error!("{:?}", e);
                    sleep(Self::ROOM_UPDATE_INTERVAL).await;
                    continue;
                }
            };

            let youtube_id_rgx: &'static regex::Regex =
                regex!(r"(?:https?://)?(?:www\.)?youtu(?:(?:\.be/)|(?:be.com/watch\?v=))(.{11,})");

            let mut new_rooms: HashMap<String, Room> = new_rooms
                .into_iter()
                .map(|mut r| {
                    if let Some(ref mut stream) = r.stream {
                        if let Some(youtube_id) =
                            youtube_id_rgx.captures(stream).and_then(|c| c.get(1))
                        {
                            *stream = youtube_id.as_str().to_string();
                        } else {
                            warn!(%stream, "Unable to get Youtube ID from stream!");
                        }
                    }
                    (r.name.clone(), r)
                })
                .collect();

            {
                let mut rooms = rooms.lock().await;
                let mut rooms_to_delete = Vec::new();

                for (room_name, room) in rooms.iter_mut() {
                    if let Some(new_room) = new_rooms.get(room_name) {
                        if room == new_room {
                            new_rooms.remove(room_name);
                            continue;
                        }

                        let mut listeners_map = listeners.lock().await;

                        if let Some(listener_ch) = listeners_map.get(room_name) {
                            if !listener_ch.is_closed() {
                                listener_ch.send(new_room.clone())?;
                            } else {
                                listeners_map.remove(room_name);
                            }
                        }

                        drop(listeners_map);

                        if room.stream == new_room.stream {
                            new_rooms.remove(room_name);
                            continue;
                        }

                        let update = match (&room.stream, &new_room.stream) {
                            (None, None) => None,
                            (None, Some(s)) => Some(RoomUpdate::Added(s.clone())),
                            (Some(s), None) => Some(RoomUpdate::Removed(s.clone())),
                            (Some(s1), Some(s2)) => {
                                Some(RoomUpdate::Changed(s1.clone(), s2.clone()))
                            }
                        };

                        if let Some(update) = update {
                            trace!(?update, ?room, "Room update detected!");
                            room_update_sender.send(update).context(here!())?;
                            *room = new_rooms.remove(room_name).unwrap();
                        } else {
                            new_rooms.remove(room_name);
                        }
                    } else {
                        if let Some(stream) = &room.stream {
                            trace!(?room, "Room removed!");
                            room_update_sender
                                .send(RoomUpdate::Removed(stream.clone()))
                                .context(here!())?;
                        }

                        rooms_to_delete.push(room_name.clone());
                    }
                }

                for room_name in rooms_to_delete.iter() {
                    rooms.remove(room_name);
                }

                for (room_name, new_room) in new_rooms.into_iter() {
                    if let Some(stream) = &new_room.stream {
                        trace!(?new_room, "Room added!");
                        room_update_sender
                            .send(RoomUpdate::Added(stream.clone()))
                            .context(here!())?;
                    }

                    rooms.insert(room_name, new_room);
                }
            }

            sleep(Self::ROOM_UPDATE_INTERVAL).await;
        }
    }
}

#[pin_project]
pub struct Listener {
    pub room: watch::Receiver<Room>,
    #[pin]
    pub stream: es::EventStream<es::HttpsConnector>,
}

impl std::fmt::Debug for Listener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Listener")
            .field("room", &self.room)
            .finish()
    }
}

impl Listener {
    pub(crate) fn new(room: watch::Receiver<Room>) -> anyhow::Result<Self> {
        let client = es::Client::for_url(&format!(
            "https://repo.mchatx.org/Listener?room={}",
            &room.borrow().name
        ))
        .map_err(|e| anyhow::anyhow!("{:?}", e))?
        .reconnect(
            es::ReconnectOptions::reconnect(true)
                .retry_initial(false)
                .delay(Duration::from_secs(1))
                .backoff_factor(2)
                .delay_max(Duration::from_secs(60))
                .build(),
        )
        .build();

        Ok(Self {
            room,
            stream: client.stream(),
        })
    }
}

impl Stream for Listener {
    type Item = EventData;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let this = self.project();
        let pinned_stream: Pin<&mut _> = this.stream;

        let next = pinned_stream.try_poll_next(cx);

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

        let data: EventData = match validate_json_bytes(data) {
            Ok(DataOrEmptyObject::Some(data)) => data,
            Ok(DataOrEmptyObject::None {}) => {
                return Poll::Pending;
            }
            Err(e) => {
                error!("{:?}", e);
                return Poll::Ready(None);
            }
        };

        {
            let room = this.room.borrow();

            if !room.allows_external_sharing || room.needs_password {
                return Poll::Pending;
            }
        }

        trace!(?data, "Event data");
        Poll::Ready(Some(data))
    }
}

#[derive(Debug, Clone)]
pub enum RoomUpdate {
    Added(String),
    Removed(String),
    Changed(String, String),
}

#[derive(Debug, Clone)]
pub enum RoomConfigUpdate {
    Locked,
    Unlocked,
    BlockedExternalSharing,
    UnblockedExternalSharing,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_test::traced_test;

    const SERVER: &str = "https://repo.mchatx.org";
    const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

    #[tokio::test]
    #[traced_test]
    async fn get_rooms() {
        let client = reqwest::ClientBuilder::new()
            .user_agent(USER_AGENT)
            .build()
            .context(here!())
            .unwrap();

        let res = try_run(|| async {
            client
                .get(format!("{}/Room", SERVER))
                .send()
                .await
                .context(here!())
        })
        .await
        .unwrap();

        println!("{:#?}", res);

        let res: Vec<Room> = validate_response(res).await.unwrap();
        println!("{:?}", res);
    }
}
