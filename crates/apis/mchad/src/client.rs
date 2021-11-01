use std::{collections::HashMap, fmt::Debug, sync::Arc, time::Duration};

use futures::StreamExt;
use miette::IntoDiagnostic;
use tokio::{
    sync::{broadcast, watch, Mutex},
    time::sleep,
};
use tracing::{debug, error, info, trace};

use crate::listener::{EventListener, Listener};

use super::types::*;
use super::util::validate_response;

#[derive(Debug)]
pub struct Client {
    pub room_updates: broadcast::Receiver<RoomUpdate>,
    rooms: Arc<Mutex<HashMap<String, Room>>>,
    listeners: Arc<Mutex<HashMap<String, watch::Sender<Room>>>>,
}

impl Client {
    const USER_AGENT: &'static str =
        concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);
    const ROOM_UPDATE_INTERVAL: Duration = Duration::from_secs(60);

    pub fn new() -> Self {
        let client = reqwest::ClientBuilder::new()
            .user_agent(Self::USER_AGENT)
            .build()
            .unwrap();

        let rooms = Arc::new(Mutex::new(HashMap::new()));
        let listeners = Arc::new(Mutex::new(HashMap::new()));
        let (room_update_tx, room_update_rx) = broadcast::channel(16);

        let room_clone = Arc::clone(&rooms);
        let listener_clone = Arc::clone(&listeners);

        tokio::spawn(async {
            if let Err(e) = Self::updater(client, room_clone, listener_clone, room_update_tx).await
            {
                error!("Error: {}", e);
            }
        });

        tokio::spawn(async {
            let room_stream = match EventListener::<serde_json::Value>::new("Room") {
                Ok(s) => s,
                Err(e) => {
                    error!("{:?}", e);
                    return;
                }
            };

            let mut room_stream = Box::pin(room_stream);

            while let Some(event) = room_stream.next().await {
                match event {
                    serde_json::Value::Null => continue,
                    serde_json::Value::Object(v) if v.is_empty() => continue,
                    _ => (),
                }

                info!(?event, "New MChad room event!");
            }
        });

        Self {
            rooms,
            listeners,
            room_updates: room_update_rx,
        }
    }

    pub async fn get_listener(&mut self, stream: &str) -> Option<Listener> {
        let rooms = self.rooms.lock().await;
        let room = rooms
            .iter()
            .find(|(_, r)| r.stream == Some(stream.into()))?
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

    async fn updater(
        client: reqwest::Client,
        rooms: Arc<Mutex<HashMap<String, Room>>>,
        listeners: Arc<Mutex<HashMap<String, watch::Sender<Room>>>>,
        room_update_sender: broadcast::Sender<RoomUpdate>,
    ) -> miette::Result<()> {
        loop {
            let res = client
                .get("https://repo.mchatx.org/Room")
                .send()
                .await
                .into_diagnostic()?;

            let new_rooms: Vec<Room> = match validate_response(res).await {
                Ok(val) => val,
                Err(e) => {
                    debug!("{:?}", e);
                    sleep(Self::ROOM_UPDATE_INTERVAL).await;
                    continue;
                }
            };

            let mut new_rooms: HashMap<String, Room> = new_rooms
                .into_iter()
                .map(|mut r| {
                    if let Some(ref mut stream) = r.stream {
                        match stream.parse() {
                            Ok(youtube_id) => {
                                *stream = youtube_id;
                            }
                            Err(_) => {
                                debug!(%stream, "Unable to get Youtube ID from stream!");
                            }
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
                                listener_ch.send(new_room.clone()).into_diagnostic()?;
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
                            room_update_sender.send(update).into_diagnostic()?;
                            *room = new_rooms.remove(room_name).unwrap();
                        } else {
                            new_rooms.remove(room_name);
                        }
                    } else {
                        if let Some(stream) = &room.stream {
                            trace!(?room, "Room removed!");
                            room_update_sender
                                .send(RoomUpdate::Removed(stream.clone()))
                                .into_diagnostic()?;
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
                            .into_diagnostic()?;
                    }

                    rooms.insert(room_name, new_room);
                }
            }

            sleep(Self::ROOM_UPDATE_INTERVAL).await;
        }
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
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
            .unwrap();

        let res = client.get(format!("{}/Room", SERVER)).send().await.unwrap();

        println!("{:#?}", res);

        let res: Vec<Room> = validate_response(res).await.unwrap();
        println!("{:?}", res);
    }
}
