use super::prelude::*;

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