use super::prelude::*;

use crate::discord::FetchDiscordData;

#[derive(Debug, Clone)]
pub struct TrackMetaData {
    pub added_by: UserId,
    pub added_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct TrackMetaDataFull {
    pub added_by: Member,
    pub added_at: DateTime<Utc>,
    pub member_colour: Option<Colour>,
}

#[derive(Debug, Clone)]
pub struct ExtractedMetaData {
    pub title: String,
    pub uploader: String,
    pub duration: Duration,
    pub thumbnail: Option<String>,
}

impl TrackMetaData {
    pub async fn fetch_data(
        &self,
        ctx: &Ctx,
        guild_id: &GuildId,
    ) -> anyhow::Result<TrackMetaDataFull> {
        let member = guild_id.member(&ctx.http, self.added_by).await?;

        Ok(TrackMetaDataFull {
            member_colour: member.colour(&ctx.cache).await,
            added_by: member,
            added_at: self.added_at,
        })
    }
}

#[async_trait]
impl FetchDiscordData<TrackMetaDataFull> for TrackMetaData {
    async fn fetch_data(self, ctx: &Ctx, guild_id: &GuildId) -> anyhow::Result<TrackMetaDataFull> {
        self.fetch_data(ctx, guild_id).await
    }
}
