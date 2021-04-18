use super::prelude::*;

#[command]
#[owners_only]
/// Unclaims all talents from a channel.
async fn unclaim(ctx: &Context, msg: &Message) -> CommandResult {
    let mut channel = msg
        .channel(&ctx.cache)
        .await
        .ok_or_else(|| anyhow!("Can't find channel!"))?
        .guild()
        .ok_or_else(|| anyhow!("Can't find guild!"))?;

    channel
        .edit(&ctx.http, |c| {
            c.topic("");
            c
        })
        .await?;

    Ok(())
}
