use std::error::Error;

use serenity::{async_trait, http::{AttachmentType}, model::{channel::Message, gateway::Ready, guild::Guild, id::ChannelId}, prelude::*};

pub struct DiscordAPI {
    client: Client,
}

impl DiscordAPI {
    pub async fn new(discord_token: &str) -> DiscordAPI {
        return DiscordAPI {
            client: Client::builder(discord_token)
                .event_handler(Handler)
                .await
                .expect("Client creation failed"),
        };
    }

    pub async fn connect(&mut self) {
        if let Err(why) = self.client.start().await {
            println!("Err with client: {:?}", why);
        }
    }
}

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
    }

    async fn guild_create(&self, ctx: Context, guild: Guild, _: bool) {
        let bot_testing = ChannelId(319017124775460865);
        let channel = guild.channels.get(&bot_testing).unwrap();

        println!(
            "{:#?}",
            guild.channels.get(&bot_testing).unwrap()
        );

        let typing = bot_testing.start_typing(&ctx.http).unwrap();

        let _ = bot_testing.send_message(&ctx.http, |m| {
            m.embed(|e| {
                e.title("Ina is based");
                e.url("https://www.youtube.com/channel/UCMwGHR0BTZuLsmjY_NT5Pwg");
                e.colour(serenity::utils::Colour::PURPLE);
                e.description("Ina vibing.");
                e.timestamp("2004-06-08T16:04:23");
                e.field("Field", "Hello this is some text in a field.", true);
                e.author(|a| {
                    a.name("Ninomae Ina'nis");
                    a.icon_url("https://static.wikia.nocookie.net/virtualyoutuber/images/4/46/Ninomae_Ina'nis_Portrait.png");
                    a.url("https://www.youtube.com/channel/UCMwGHR0BTZuLsmjY_NT5Pwg");

                    a
                });

                e.footer(|f| {
                    f.text("Hello this is some text in a footer.");
                    
                    f
                });

                e
            });

            m
        }).await.unwrap();

        typing.stop().unwrap();
    }
}
