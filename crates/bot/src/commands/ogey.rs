use super::prelude::*;

#[poise::command(slash_command, prefix_command)]
/// rrat
pub(crate) async fn ogey(ctx: Context<'_>) -> anyhow::Result<()> {
    ctx.send(|m| {
        m.ephemeral(true)
            .content("rrat <:pekoSlurp:824792426530734110>")
    })
    .await?;

    Ok(())
}
