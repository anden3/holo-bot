#![allow(dead_code)]

use anyhow::{anyhow, Context as _};
use futures::StreamExt;
use poise::{ApplicationCommandOrAutocompleteInteraction, CreateReply, ReplyHandle};
use serenity::{
    builder::CreateEmbed,
    model::{
        channel::{Message, ReactionType},
        interactions::{message_component::ButtonStyle, InteractionResponseType},
    },
    utils::Colour,
};
use tokio::{sync::oneshot, time::Duration};
use tokio_util::sync::CancellationToken;
use tracing::error;
use utility::here;

use crate::commands::Context;

pub type ElementFormatter<'a, D> = Box<dyn Fn(&D, &[String]) -> String + Send + Sync>;
pub type EmbedFormatter<'a, D> = Box<dyn Fn(&D, &Vec<String>) -> CreateEmbed + Send + Sync>;

pub struct PaginatedList<'a, D> {
    title: Option<String>,
    layout: PageLayout,

    data: &'a [D],
    format_func: Option<ElementFormatter<'a, D>>,
    embed_func: Option<EmbedFormatter<'a, D>>,

    show_page_count: ShowPageCount,
    page_change_perm: PageChangePermission,

    timeout: Duration,
    token: Option<CancellationToken>,
    message_sender: Option<oneshot::Sender<Message>>,

    delete_when_dropped: bool,
    params: Vec<String>,
}

pub enum PageLayout {
    Standard {
        items_per_page: usize,
    },
    Chunked {
        chunk_size: usize,
        chunks_per_page: usize,
    },
}

pub enum ShowPageCount {
    Always,
    WhenSeveralPages,
    Never,
}

pub enum PageChangePermission {
    Interactor,
    Everyone,
}

enum FormattedData<'a, D> {
    Standard(&'a [D]),
    Chunked(Vec<(usize, &'a [D])>),
}

impl<'a, D: std::fmt::Debug> PaginatedList<'a, D> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn title<T: ToString>(&'_ mut self, title: T) -> &'_ mut Self {
        self.title = Some(title.to_string());
        self
    }

    pub fn layout(&'_ mut self, layout: PageLayout) -> &'_ mut Self {
        self.layout = layout;
        self
    }

    pub fn data(&'_ mut self, data: &'a [D]) -> &'_ mut Self {
        self.data = data;
        self
    }

    pub fn embed(&'_ mut self, embed: EmbedFormatter<'a, D>) -> &'_ mut Self {
        self.embed_func = Some(embed);
        self
    }

    pub fn format(&'_ mut self, format: ElementFormatter<'a, D>) -> &'_ mut Self {
        self.format_func = Some(format);
        self
    }

    pub fn show_page_count(&'_ mut self, show_page_count: ShowPageCount) -> &'_ mut Self {
        self.show_page_count = show_page_count;
        self
    }

    pub fn page_change_permission(&'_ mut self, permission: PageChangePermission) -> &'_ mut Self {
        self.page_change_perm = permission;
        self
    }

    pub fn timeout(&'_ mut self, timeout: Duration) -> &'_ mut Self {
        self.timeout = timeout;
        self
    }

    pub fn token(&'_ mut self, token: CancellationToken) -> &'_ mut Self {
        self.token = Some(token);
        self
    }

    pub fn get_message(&'_ mut self, channel: oneshot::Sender<Message>) -> &'_ mut Self {
        self.message_sender = Some(channel);
        self
    }

    pub fn delete_when_dropped(&'_ mut self, delete: bool) -> &'_ mut Self {
        self.delete_when_dropped = delete;
        self
    }

    pub fn params(&'_ mut self, params: &[&str]) -> &'_ mut Self {
        self.params = params.iter().map(|p| p.to_string()).collect();
        self
    }

    pub async fn display(&'_ mut self, ctx: Context<'_>) -> anyhow::Result<()> {
        let mut current_page: i32 = 1;

        if self.data.is_empty() {
            ctx.send(|m| m.ephemeral(true).content("No data to display."))
                .await?;
            return Ok(());
        }

        let typing_guard = ctx.defer_or_broadcast().await?;

        let (data, required_pages) = match self.layout {
            PageLayout::Standard { items_per_page } => (
                FormattedData::Standard(self.data),
                ((self.data.len() as f32) / items_per_page as f32).ceil() as usize,
            ),
            PageLayout::Chunked {
                chunk_size,
                chunks_per_page,
            } => (
                FormattedData::Chunked(
                    self.data.chunks(chunk_size).enumerate().collect::<Vec<_>>(),
                ),
                ((self.data.len() as f32) / (chunk_size * chunks_per_page) as f32).ceil() as usize,
            ),
        };

        let token = self.token.take().unwrap_or_default();
        let message_sender = self.message_sender.take();

        let mut reply_handle = {
            let reply_handle = self
                .create_page(&data, current_page as usize, required_pages, ctx, None)
                .await;

            match reply_handle {
                Ok(handle) => handle,
                Err(err) => {
                    error!("{err:?}");
                    return Err(anyhow!(err)).context(here!());
                }
            }
        };

        // TODO: Replace by cloning ReplyHandle and calling .message() when that gets implemented.
        let message = match &reply_handle {
            ReplyHandle::Known(msg) => *msg.clone(),
            ReplyHandle::Unknown { http, interaction } => {
                match interaction.get_interaction_response(http).await {
                    Ok(msg) => msg,
                    Err(err) => {
                        error!("{err:?}");
                        return Err(anyhow!(err)).context(here!());
                    }
                }
            }
            ReplyHandle::Autocomplete => unreachable!(),
        };

        if let Some(channel) = message_sender {
            channel
                .send(message.clone())
                .map_err(|m| anyhow!("Could not send message: {}.", m.id))
                .context(here!())?;
        }

        if required_pages == 1 {
            return Ok(());
        }

        let page_turn_stream = message
            .await_component_interactions(&ctx.discord().shard)
            .timeout(self.timeout);

        let page_turn_stream = match self.page_change_perm {
            PageChangePermission::Interactor => page_turn_stream.author_id(ctx.author().id),
            _ => page_turn_stream,
        };

        drop(typing_guard);

        let mut page_turn_stream = Box::pin(page_turn_stream.build());

        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    break;
                }
                page_turn = page_turn_stream.next() => {
                    let page_turn = match &page_turn {
                        Some(r) => r,
                        None => break,
                    };

                    match page_turn.data.custom_id.as_str() {
                        "back" => {
                            current_page -= 1;

                            if current_page < 1 {
                                current_page = required_pages as i32;
                            }
                        }
                        "forward" => {
                            current_page += 1;

                            if current_page > required_pages as i32 {
                                current_page = 1;
                            }
                        }
                        _ => continue,
                    }

                    page_turn.create_interaction_response(&ctx.discord().http, |r| {
                        r.kind(InteractionResponseType::DeferredUpdateMessage)
                    }).await.context(here!())?;

                    reply_handle = self.create_page(
                        &data, current_page as usize,
                        required_pages,
                        ctx, Some(reply_handle)
                    )
                    .await?;
                }
            }
        }

        if let Context::Application(app_ctx) = ctx {
            if let ApplicationCommandOrAutocompleteInteraction::ApplicationCommand(interaction) =
                app_ctx.interaction
            {
                if self.delete_when_dropped {
                    interaction
                        .delete_original_interaction_response(&ctx.discord().http)
                        .await
                        .context(here!())?;
                } else {
                    interaction
                        .edit_original_interaction_response(&ctx.discord().http, |e| {
                            e.components(|c| c)
                        })
                        .await
                        .context(here!())?;
                }
            }
        }

        Ok(())
    }

    async fn create_page<'b>(
        &'b self,
        data: &FormattedData<'b, D>,
        page: usize,
        required_pages: usize,
        ctx: Context<'b>,
        reply_handle: Option<ReplyHandle<'b>>,
    ) -> anyhow::Result<poise::ReplyHandle<'b>> {
        let page = {
            let mut m = CreateReply::default();

            if required_pages > 1 {
                m.components(|c| {
                    c.create_action_row(|r| {
                        r.create_button(|b| {
                            b.style(ButtonStyle::Secondary)
                                .label("Back")
                                .custom_id("back")
                                .emoji(ReactionType::Unicode("👈".to_string()))
                        })
                        .create_button(|b| {
                            b.style(ButtonStyle::Secondary)
                                .label("Forward")
                                .custom_id("forward")
                                .emoji(ReactionType::Unicode("👉".to_string()))
                        })
                    })
                });
            }

            if let Some(func) = &self.embed_func {
                match (&self.layout, data) {
                    (PageLayout::Standard { items_per_page }, FormattedData::Standard(d)) => {
                        let embed_page = d
                            .iter()
                            .skip(((page - 1) as usize) * *items_per_page)
                            .take(*items_per_page);

                        m.embeds.clear();

                        for embed in embed_page {
                            m.embed(|m| {
                                *m = func(embed, &self.params);
                                m
                            });
                        }
                    }
                    _ => error!("Invalid layout and data format found!"),
                }
            } else {
                m.embed(|e| {
                    e.colour(Colour::new(6_282_735));

                    if let Some(title) = &self.title {
                        e.title(title);
                    }

                    match (&self.layout, data) {
                        (PageLayout::Standard { items_per_page }, FormattedData::Standard(d)) => {
                            if let Some(func) = &self.format_func {
                                e.description(
                                    d.iter()
                                        .skip(((page - 1) as usize) * *items_per_page)
                                        .take(*items_per_page)
                                        .fold(String::new(), |mut acc, element| {
                                            acc += func(element, &self.params).as_str();
                                            acc
                                        }),
                                );
                            }
                        }
                        (
                            PageLayout::Chunked {
                                chunk_size,
                                chunks_per_page,
                            },
                            FormattedData::Chunked(d),
                        ) => {
                            e.fields(
                                d.iter()
                                    .skip((page - 1) * chunks_per_page)
                                    .take(*chunks_per_page)
                                    .map(|(i, chunk)| {
                                        (
                                            format!(
                                                "{}-{}",
                                                i * chunk_size + 1,
                                                i * chunk_size + chunk.len()
                                            ),
                                            chunk.iter().fold(String::new(), |mut acc, element| {
                                                acc += match &self.format_func {
                                                    Some(func) => func(element, &self.params),
                                                    None => format!("{:?}", element),
                                                }
                                                .as_str();
                                                acc
                                            }),
                                            true,
                                        )
                                    }),
                            );
                        }
                        _ => error!("Invalid layout and data format found!"),
                    }

                    match self.show_page_count {
                        ShowPageCount::Always => {
                            e.footer(|f| f.text(format!("Page {} of {}", page, required_pages)));
                        }
                        ShowPageCount::WhenSeveralPages if required_pages > 1 => {
                            e.footer(|f| f.text(format!("Page {} of {}", page, required_pages)));
                        }
                        _ => (),
                    }
                    e
                });
            }

            m
        };

        match reply_handle {
            Some(handle) => handle
                .edit(ctx, |r| {
                    *r = page;
                    r
                })
                .await
                .map(|_| handle)
                .map_err(|e| e.into()),

            None => ctx
                .send(|r| {
                    *r = page;
                    r
                })
                .await
                .map_err(|e| e.into()),
        }
    }
}

impl<'a, D> Default for PaginatedList<'a, D> {
    fn default() -> Self {
        Self {
            title: None,
            layout: PageLayout::Standard { items_per_page: 5 },
            data: &[],
            format_func: None,
            embed_func: None,
            show_page_count: ShowPageCount::WhenSeveralPages,
            page_change_perm: PageChangePermission::Everyone,
            timeout: Duration::from_secs(14 * 60),
            token: None,
            message_sender: None,
            delete_when_dropped: false,
            params: Vec::new(),
        }
    }
}
