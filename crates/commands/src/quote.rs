use serenity::builder::CreateEmbed;
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
            req new_quote: String,
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

#[derive(Debug)]
enum SearchCriteria {
    User(String),
    Content(String),
}

#[allow(clippy::unnecessary_operation)]
#[interaction_cmd]
pub async fn quote(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    config: &Config,
) -> anyhow::Result<()> {
    show_deferred_response(interaction, ctx, false).await?;

    match_sub_commands! {
        "add" => |quote: req String| {
            add_quote(ctx, interaction, config, quote).await?;
        }

        "remove" => |id: req usize| {
            remove_quote(ctx, interaction, id).await?;
        }

        "edit" => |id: req usize, quote: req String| {{
            edit_quote(ctx, interaction, config, id, quote).await?;
        }}

        "get" => |id: req usize| {
            get_quote(ctx, interaction, config, id).await?;
        }

        "search by_user" => |user: req String| {
            search_for_quote(ctx, interaction, config, SearchCriteria::User(user)).await?;
        }

        "search by_content" => |content: req String| {
            search_for_quote(ctx, interaction, config, SearchCriteria::Content(content)).await?;
        }
    }

    Ok(())
}

#[instrument(skip(ctx, interaction, config))]
async fn add_quote(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    config: &Config,
    quote: String,
) -> anyhow::Result<()> {
    let quote = match Quote::from_message(&quote, &config.talents) {
        Ok(q) => q,
        Err(err) => {
            interaction
                .edit_original_interaction_response(&ctx.http, |e| {
                    e.content(format!("Error: {}", err))
                })
                .await?;
            return Err(err);
        }
    };

    let mut embed = quote.as_embed(&config.talents)?;

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

    Ok(())
}

#[instrument(skip(ctx, interaction))]
async fn remove_quote(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    quote_id: usize,
) -> anyhow::Result<()> {
    let mut data = ctx.data.write().await;
    let quotes = data.get_mut::<Quotes>().unwrap();

    if quotes.get(quote_id).is_some() {
        quotes.remove(quote_id);
    }

    std::mem::drop(data);

    interaction
        .edit_original_interaction_response(&ctx.http, |e| {
            e.content(format!("Quote {} removed!", quote_id))
        })
        .await?;

    Ok(())
}

#[instrument(skip(ctx, interaction, config))]
async fn edit_quote(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    config: &Config,
    quote_id: usize,
    new_quote: String,
) -> anyhow::Result<()> {
    let data = ctx.data.read().await;
    let quotes = data.get::<Quotes>().unwrap();

    if quotes.get(quote_id).is_none() {
        interaction
            .edit_original_interaction_response(&ctx.http, |e| {
                e.content(format!("No quote with the ID {} found!", quote_id))
            })
            .await?;
        return Ok(());
    }

    let quote = match Quote::from_message(&new_quote, &config.talents) {
        Ok(q) => q,
        Err(err) => {
            interaction
                .edit_original_interaction_response(&ctx.http, |e| {
                    e.content(format!("Error: {}", err))
                })
                .await?;

            return Err(err);
        }
    };

    let mut data = ctx.data.write().await;
    let quotes = data.get_mut::<Quotes>().unwrap();

    quotes[quote_id] = quote;

    interaction
        .edit_original_interaction_response(&ctx.http, |e| {
            e.content(format!("Quote {} edited!", quote_id))
        })
        .await?;

    Ok(())
}

#[instrument(skip(ctx, interaction, config))]
async fn get_quote(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    config: &Config,
    quote_id: usize,
) -> anyhow::Result<()> {
    let data = ctx.data.read().await;
    let quotes = data.get::<Quotes>().unwrap();

    let quote = match quotes.get(quote_id) {
        Some(q) => q,
        None => {
            interaction
                .edit_original_interaction_response(&ctx.http, |e| {
                    e.content(format!("No quote with the ID {} found!", quote_id))
                })
                .await?;

            return Ok(());
        }
    };

    let mut embed = quote.as_embed(&config.talents)?;
    std::mem::drop(data);

    interaction
        .edit_original_interaction_response(&ctx.http, |e| {
            embed.footer(|f| f.text(format!("ID: {}", quote_id)));

            e.add_embed(embed)
        })
        .await?;

    Ok(())
}

#[instrument(skip(ctx, interaction, config))]
async fn search_for_quote(
    ctx: &Ctx,
    interaction: &ApplicationCommandInteraction,
    config: &Config,
    search_criteria: SearchCriteria,
) -> anyhow::Result<()> {
    let matching_quotes = match search_criteria {
        SearchCriteria::User(ref user) => {
            let user = user.trim().to_lowercase();

            let user = config
                .talents
                .iter()
                .find(|u| u.name.to_lowercase().contains(&user))
                .ok_or_else(|| anyhow!("No talent found with the name {}!", user));

            let user = match user {
                Ok(u) => u,
                Err(err) => {
                    interaction
                        .edit_original_interaction_response(&ctx.http, |e| {
                            e.content(format!("Error: {}", err))
                        })
                        .await?;
                    return Err(err);
                }
            };

            let data = ctx.data.read().await;
            let quotes = data.get::<Quotes>().unwrap();

            quotes
                .iter()
                .filter(|q| q.lines.iter().any(|l| l.user == user.name))
                .cloned()
                .collect::<Vec<_>>()
        }

        SearchCriteria::Content(ref content) => {
            let normalized_content = content.trim().to_lowercase();
            let data = ctx.data.read().await;
            let quotes = data.get::<Quotes>().unwrap();

            quotes
                .iter()
                .filter(|q| q.lines.iter().any(|l| l.line.contains(&normalized_content)))
                .cloned()
                .collect::<Vec<_>>()
        }
    };

    if matching_quotes.is_empty() {
        interaction
            .edit_original_interaction_response(&ctx.http, |e| {
                e.content("No matching quotes found!")
            })
            .await?;

        return Ok(());
    }

    let title = match &search_criteria {
        SearchCriteria::User(user) => format!("Quotes by {}", user),
        SearchCriteria::Content(content) => format!("Quotes containing \"{}\"", content),
    };

    PaginatedList::new()
        .title(title)
        .data(&matching_quotes)
        .embed(Box::new(|q, _| {
            let mut embed = CreateEmbed::default();

            embed.fields(
                q.lines
                    .iter()
                    .map(|l| (l.user.clone(), l.line.clone(), false)),
            );

            embed
        }))
        .display(ctx, interaction)
        .await?;

    Ok(())
}
