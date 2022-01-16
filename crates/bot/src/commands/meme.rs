use super::prelude::*;

use apis::meme_api::MemeFont;

#[poise::command(
    slash_command,
    check = "meme_creation_enabled",
    member_cooldown = 60,
    required_permissions = "ATTACH_FILES"
)]
/// Generate a meme, peko!
pub(crate) async fn meme(
    ctx: Context<'_>,
    #[description = "The meme template to use."]
    #[autocomplete = "autocomplete_template"]
    template: String,
    #[description = "The captions in the meme, separated by |."] captions: String,
    #[description = "Which font to use?"] font: Option<MemeFont>,
    #[description = "Maximum font size in pixels"] max_font_size: Option<u32>,
) -> anyhow::Result<()> {
    let font = font.unwrap_or_default();
    let max_font_size = max_font_size.unwrap_or(50);
    let mut captions = captions.split('|').collect::<Vec<_>>();

    ctx.defer().await?;

    let meme_api = {
        let data = ctx.data();
        let read_lock = data.data.read().await;

        match read_lock.meme_creator.as_ref() {
            Some(meme_api) => meme_api.clone(),
            None => {
                return Err(anyhow!(
                    "Meme creator is not enabled. Please enable it in the config."
                ));
            }
        }
    };

    let meme = {
        let arc = meme_api.get_popular_memes().await.context(here!())?;
        let memes = arc.read().await;

        match memes
            .iter()
            .find(|m| m.name.to_ascii_lowercase() == template.to_ascii_lowercase())
        {
            Some(meme) => meme.clone(),
            None => {
                return Err(anyhow!("No meme found with the name `{template}`"));
            }
        }
    };

    match captions.len().cmp(&meme.box_count) {
        std::cmp::Ordering::Less => {
            captions.extend(std::iter::repeat("").take(meme.box_count - captions.len()));
        }
        std::cmp::Ordering::Greater => {
            captions.truncate(meme.box_count);
        }
        _ => (),
    }

    let captions = captions
        .into_iter()
        .map(|c| c.trim().to_owned())
        .collect::<Vec<_>>();

    let url = meme_api
        .create_meme(&meme, captions, font, max_font_size as i64)
        .await?;

    ctx.send(|m| {
        m.embed(|e| {
            e.colour(Colour::new(6_282_735));
            e.image(url)
        })
    })
    .await
    .context(here!())?;

    Ok(())
}

async fn meme_creation_enabled(ctx: Context<'_>) -> anyhow::Result<bool> {
    Ok(ctx.data().config.meme_creation.enabled)
}

async fn autocomplete_template(ctx: Context<'_>, partial: String) -> impl Iterator<Item = String> {
    let partial = partial.to_ascii_lowercase();

    let data = ctx.data();
    let read_lock = data.data.read().await;

    let memes = read_lock
        .meme_creator
        .as_ref()
        .unwrap()
        .get_popular_memes()
        .await;

    let meme_names = match memes {
        Ok(memes) => memes
            .read()
            .await
            .iter()
            .map(|m| m.name.to_owned())
            .collect(),
        Err(_) => Vec::new(),
    };

    meme_names
        .into_iter()
        .filter_map(move |m| m.to_ascii_lowercase().contains(&partial).then(|| m))
}
