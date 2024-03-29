use std::str::FromStr;

use tokio::time::Duration;

use super::prelude::*;

use apis::meme_api::{Meme, MemeApi, MemeFont};

interaction_setup! {
    name = "meme",
    group = "fun",
    description = "Create a meme peko",
    options = [
        //! Which font to use?
        font: String = enum MemeFont,
        //! Maximum font size in pixels.
        max_font_size: Integer
    ],
    restrictions = [
        allowed_roles = [
            "Admin",
            "Moderator",
            "Moderator (JP)",
            824337391006646343
        ]
    ]
}

#[allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap
)]
#[interaction_cmd]
async fn meme(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    config: &Config,
) -> anyhow::Result<()> {
    parse_interaction_options!(interaction.data, [font: enum MemeFont = MemeFont::Impact, max_font_size: i64 = 50]);

    interaction
        .create_interaction_response(&ctx.http, |r| {
            r.kind(InteractionResponseType::DeferredChannelMessageWithSource)
                .interaction_response_data(|d| d.content("Loading..."))
        })
        .await
        .context(here!())?;

    let data = ctx.data.read().await;
    let meme_api = data.get::<MemeApi>().unwrap().clone();
    let mut message_recv = data.get::<MessageSender>().unwrap().subscribe();
    std::mem::drop(data);

    let arc = meme_api.get_popular_memes().await.context(here!())?;
    let memes = arc.read().await;

    let mut matching_meme: Option<&Meme> = None;

    let token = CancellationToken::new();
    let (msg_send, msg_recv) = tokio::sync::oneshot::channel::<Message>();

    let mut paginated_list = PaginatedList::new();
    let list = paginated_list
        .title("Pick your meme template!")
        .data(&memes)
        .layout(PageLayout::Chunked {
            chunk_size: 10,
            chunks_per_page: 3,
        })
        .format(Box::new(|m, _| format!("{}\r\n", m.name)))
        .timeout(Duration::from_secs(60 * 5))
        .token(token.child_token())
        .get_message(msg_send)
        .display(interaction, ctx);

    tokio::pin!(list);
    tokio::pin!(msg_recv);

    let mut list_msg = None;

    loop {
        tokio::select! {
            msg = &mut msg_recv, if list_msg.is_none() => {
                list_msg = Some(msg.context(here!())?);
            }
            _ = &mut list => {
                break;
            }
            Ok(MessageUpdate::Sent(msg)) = message_recv.recv() => {
                if msg.author.id != interaction.member.as_ref().unwrap().user.id {
                    continue;
                }

                if msg.channel_id != interaction.channel_id {
                    continue;
                }

                let text = msg.content.trim();

                matching_meme = match text.parse::<usize>() {
                    Ok(num) => match &memes.get(num - 1) {
                        Some(meme) => Some(meme),
                        None => continue,
                    },
                    Err(_) => match memes
                        .iter()
                        .find(|m| m.name.to_ascii_lowercase() == text.to_ascii_lowercase())
                    {
                        Some(meme) => Some(meme),
                        None => continue,
                    },
                };
                msg.delete(&ctx).await?;
                break;
            }
        }
    }

    let message = match list_msg {
        Some(msg) => msg,
        None => return Err(anyhow!("Failed to get message from list.")).context(here!()),
    };

    let meme = match matching_meme {
        Some(meme) => meme,
        None => return Ok(()),
    };

    message.delete_reactions(&ctx).await.context(here!())?;

    let _message = interaction
        .edit_original_interaction_response(&ctx.http, |r| {
            r.create_embed(|e| {
                e.title(meme.name.to_owned());
                e.description(format!(
                    "Meme has {} text boxes. Please type each caption on a separate line.",
                    meme.box_count
                ));
                e.colour(Colour::new(6_282_735));
                e.image(meme.url.to_owned())
            })
        })
        .await?;

    let mut captions = Vec::with_capacity(meme.box_count);

    while let Ok(Ok(update)) =
        tokio::time::timeout(Duration::from_secs(60 * 10), message_recv.recv()).await
    {
        if let MessageUpdate::Sent(msg) = update {
            if msg.author.id != interaction.member.as_ref().unwrap().user.id {
                continue;
            }

            if msg.channel_id != interaction.channel_id {
                continue;
            }

            captions.extend(
                msg.content
                    .trim()
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .take(meme.box_count)
                    .map(std::borrow::ToOwned::to_owned),
            );

            msg.delete(&ctx).await.context(here!())?;

            if captions.len() == meme.box_count {
                break;
            }
        }
    }

    if captions.len() < meme.box_count {
        return Ok(());
    }

    let url = meme_api
        .create_meme(meme, captions, font, max_font_size)
        .await?;

    let _message = interaction
        .edit_original_interaction_response(&ctx.http, |r| {
            r.create_embed(|e| {
                e.colour(Colour::new(6_282_735));
                e.image(url)
            })
        })
        .await
        .context(here!())?;

    Ok(())
}
