use serenity::{async_trait, model::id::GuildId};

use crate::types::Ctx;

#[async_trait]
pub trait FetchDiscordData<T> {
    async fn fetch_data(self, ctx: &Ctx, guild_id: &GuildId) -> anyhow::Result<T>;
}
