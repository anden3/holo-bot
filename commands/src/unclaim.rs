use super::prelude::*;

#[command]
#[owners_only]
/// Unclaims all talents from a channel.
async fn unclaim(ctx: &Ctx, msg: &Message) -> CommandResult {
    let mut channel = msg
        .channel(&ctx.cache)
        .await
        .ok_or_else(|| anyhow!("Can't find channel!"))
        .context(here!())?
        .guild()
        .ok_or_else(|| anyhow!("Can't find guild!"))
        .context(here!())?;

    channel
        .edit(&ctx.http, |c| {
            c.topic("");
            c
        })
        .await
        .context(here!())?;

    Ok(())
}
