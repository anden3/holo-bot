use utility::config::Quote;

use super::prelude::*;

interaction_setup! {
    name = "quote",
    group = "utility",
    description =  "Quote-related commands.",
    options = [
        //! Add new quote.
        add: SubCommand = [
            //! The quote to add.
            req quote: String,
        ],
        //! Remove quote.
        remove: SubCommand = [
            //! ID of the quote to remove.
            req id: Integer,
        ],
        //! Edit quote.
        edit: SubCommand = [
            //! ID of the quote to edit.
            req id: Integer,
            //! The replacement quote.
            new_quote: String,
        ],
        //! Get quote by ID.
        get: SubCommand = [
            //! ID of the quote to get.
            req id: Integer,
        ],
        //! Find matching quotes.
        search: SubCommandGroup = [
            //! Find quotes with talent.
            by_user: SubCommand = [
                //! The name of the user.
                req user: String,
            ],
            //! Find quotes containing text.
            by_content: SubCommand = [
                //! The text to search.
                req search: String,
            ],
        ]
    ],
    restrictions = [
        rate_limit = 2 in 1 minute for user
    ]
}

#[interaction_cmd]
pub async fn quote(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    config: &Config,
) -> anyhow::Result<()> {
    show_deferred_response(interaction, ctx, false).await?;

    match_sub_commands! {
        "add" => |quote: req String| {
            let quote = match Quote::from_message(&quote, &config.users) {
                Ok(q) => q,
                Err(err) => {
                    interaction
                        .edit_original_interaction_response(&ctx.http, |e| {
                            e.content(format!("Error: {}", err))
                        })
                        .await?;
                    break;
                }
            };

            let mut embed = quote.as_embed(&config.users)?;

            let mut data = ctx.data.write().await;
            let quotes = data.get_mut::<Quotes>().unwrap();

            quotes.push(quote.clone());
            let id = quotes.len();
            std::mem::drop(data);

            interaction
                .edit_original_interaction_response(&ctx.http, |e| {
                    embed.author(|a| a.name("Quote added!"));
                    embed.footer(|f| f.text(format!("ID: {}", id)));

                    e.add_embed(embed)
                })
                .await?;
        }

        "remove" => |id: req usize| {
            let mut data = ctx.data.write().await;
            let quotes = data.get_mut::<Quotes>().unwrap();

            if quotes.get(id).is_some() {
                quotes.remove(id);
            }

            std::mem::drop(data);

            interaction
                .edit_original_interaction_response(&ctx.http, |e| {
                    e.content(format!("Quote {} removed!", id))
                })
                .await?;
        }

        "edit" => |id: req usize, quote: String| {{
            let quote = match quote {
                Some(q) => q,
                None => break,
            };

            let data = ctx.data.read().await;
            let quotes = data.get::<Quotes>().unwrap();

            if quotes.get(id).is_none() {
                interaction
                    .edit_original_interaction_response(&ctx.http, |e| {
                        e.content(format!("No quote with the ID {} found!", id))
                    })
                    .await?;
                break;
            }

            let quote = match Quote::from_message(&quote, &config.users) {
                Ok(q) => q,
                Err(err) => {
                    interaction
                        .edit_original_interaction_response(&ctx.http, |e| {
                            e.content(format!("Error: {}", err))
                        })
                        .await?;
                    return Ok(());
                }
            };

            let mut data = ctx.data.write().await;
            let quotes = data.get_mut::<Quotes>().unwrap();

            quotes[id] = quote;

            interaction
                .edit_original_interaction_response(&ctx.http, |e| {
                    e.content(format!("Quote {} edited!", id))
                })
                .await?;
        }}

        "get" => |id: req usize| {
            let data = ctx.data.read().await;
            let quotes = data.get::<Quotes>().unwrap();

            let quote = match quotes.get(id) {
                Some(q) => q,
                None => {
                    interaction
                        .edit_original_interaction_response(&ctx.http, |e| {
                            e.content(format!("No quote with the ID {} found!", id))
                        })
                        .await?;
                    break;
                }
            };

            let mut embed = quote.as_embed(&config.users)?;
            std::mem::drop(data);

            interaction
                .edit_original_interaction_response(&ctx.http, |e| {
                    embed.footer(|f| f.text(format!("ID: {}", id)));

                    e.add_embed(embed)
                })
                .await?;
        }

        "search by_user" => |user: req String| {
            let user = user.trim().to_lowercase();

            let user = config
                .users
                .iter()
                .find(|u| u.name.to_lowercase().contains(&user))
                .ok_or_else(|| anyhow!("No talent found with the name {}!", user));

            let user = match user {
                Ok(u) => u,
                Err(err) => {
                    interaction
                        .edit_original_interaction_response(
                            &ctx.http,
                            |e| e.content(format!("Error: {}", err)),
                        )
                        .await?;
                    break;
                }
            };

            let _matching_quotes = {
                let data = ctx.data.read().await;
                let quotes = data.get::<Quotes>().unwrap();

                quotes
                    .iter()
                    .filter(|q| q.lines.iter().any(|l| l.user == user.name))
                    .cloned()
                    .collect::<Vec<_>>()
            };
        }

        "search by_content" => |_search: req String| {

        }
    }

    Ok(())
}
