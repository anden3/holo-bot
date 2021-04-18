use super::prelude::*;

#[command]
#[usage = "<talent_name>[|talent2_name|...]"]
#[example = "Rikka"]
#[example = "Tokino Sora | Sakura Miko"]
#[owners_only]
/// Claims the channel for some Hololive talents.
async fn claim(ctx: &Context, msg: &Message) -> CommandResult {
    let mut args = Args::new(
        msg.content_safe(&ctx.cache)
            .await
            .get(6..)
            .ok_or_else(|| anyhow!("Can't parse arguments."))?,
        &[Delimiter::Single('|')],
    );
    args.trimmed();

    let mut talents = Vec::new();

    let data = ctx.data.read().await;
    let config = data.get::<Config>().unwrap();

    for arg in args.iter::<String>() {
        if let Ok(talent_name) = arg {
            debug!("{}", talent_name);

            if let Some(user) = config
                .users
                .iter()
                .find(|u| u.display_name.to_lowercase() == talent_name.trim().to_lowercase())
            {
                talents.push(user);
            }
        }
    }

    let mut channel = msg
        .channel(&ctx.cache)
        .await
        .ok_or_else(|| anyhow!("Can't find channel!"))?
        .guild()
        .ok_or_else(|| anyhow!("Can't find guild!"))?;

    channel
        .edit(&ctx.http, |c| {
            c.topic(talents.iter().fold(String::new(), |acc, u| acc + &u.emoji));
            c
        })
        .await?;

    Ok(())
}
