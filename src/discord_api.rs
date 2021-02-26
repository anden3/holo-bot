use std::sync::Arc;

use serenity::{builder::CreateMessage, model::id::ChannelId, prelude::*, CacheAndHttp};

pub struct DiscordAPI {
    pub cache_and_http: Arc<CacheAndHttp>,
}

impl DiscordAPI {
    pub async fn new(discord_token: &str) -> DiscordAPI {
        let client = Client::builder(discord_token)
            .await
            .expect("[DISCORD] Client creation failed");

        return DiscordAPI {
            cache_and_http: client.cache_and_http.clone(),
        };
    }

    pub async fn send_message<'a, F>(&self, channel: ChannelId, f: F)
    where
        for<'b> F: FnOnce(&'b mut CreateMessage<'a>) -> &'b mut CreateMessage<'a>,
    {
        let _ = channel
            .send_message(&self.cache_and_http.http, f)
            .await
            .unwrap();
    }
}
