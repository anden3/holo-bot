use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
};

use apis::holo_api::{Livestream, StreamUpdate};
use futures::StreamExt;
use rusqlite::Connection;
use serenity::{
    builder::CreateEmbed,
    framework::standard::{Configuration, DispatchError, Reason},
    model::{
        channel::ReactionType,
        id::{CommandId, EmojiId, GuildId},
        interactions::{ButtonStyle, InteractionData},
    },
    prelude::TypeMapKey,
};
use tokio::{
    sync::{broadcast, oneshot, watch, Mutex},
    time::Duration,
};

pub use super::interactions::RegisteredInteraction;

use super::prelude::*;

use utility::{
    client_data_types,
    config::{EmojiStats, LoadFromDatabase, Quote, SaveToDatabase},
    wrap_type_aliases,
};

pub use tokio_util::sync::CancellationToken;

wrap_type_aliases!(
    Quotes = Vec<Quote>,
    DbHandle = Mutex<rusqlite::Connection>,
    EmojiUsage = HashMap<EmojiId, EmojiStats>,
    StreamIndex = watch::Receiver<HashMap<u32, Livestream>>,
    StreamUpdateTx = broadcast::Sender<StreamUpdate>,
    MessageSender = broadcast::Sender<MessageUpdate>,
    ClaimedChannels = HashMap<ChannelId, (Livestream, CancellationToken)>,
    RegisteredInteractions = HashMap<GuildId, HashMap<CommandId, RegisteredInteraction>>
);

client_data_types!(
    Quotes,
    DbHandle,
    EmojiUsage,
    StreamIndex,
    StreamUpdateTx,
    MessageSender,
    ClaimedChannels,
    RegisteredInteractions
);

impl DerefMut for Quotes {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl DerefMut for EmojiUsage {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl DerefMut for ClaimedChannels {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl DerefMut for RegisteredInteractions {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Default for ClaimedChannels {
    fn default() -> Self {
        Self(HashMap::new())
    }
}

impl Default for RegisteredInteractions {
    fn default() -> Self {
        Self(HashMap::new())
    }
}

impl From<Vec<Quote>> for Quotes {
    fn from(vec: Vec<Quote>) -> Self {
        Self(vec)
    }
}

impl SaveToDatabase for Quotes {
    fn save_to_database(&self, handle: &Connection) -> anyhow::Result<()> {
        let mut stmt = handle.prepare_cached("INSERT OR REPLACE INTO Quotes (quote) VALUES (?)")?;

        let tx = handle.unchecked_transaction()?;

        for quote in &self.0 {
            stmt.execute([quote])?;
        }

        tx.commit()?;
        Ok(())
    }
}

impl LoadFromDatabase for Quotes {
    type Item = Quote;

    fn load_from_database(handle: &Connection) -> anyhow::Result<Vec<Self::Item>> {
        let mut stmt = handle
            .prepare("SELECT quote FROM Quotes")
            .context(here!())?;

        let results = stmt.query_and_then([], |row| -> anyhow::Result<Self::Item> {
            row.get(0).map_err(|e| anyhow!(e))
        })?;

        results.collect()
    }
}

impl SaveToDatabase for EmojiUsage {
    fn save_to_database(&self, handle: &Connection) -> anyhow::Result<()> {
        let mut stmt = handle.prepare_cached(
            "INSERT OR REPLACE INTO emoji_usage (emoji_id, text_count, reaction_count) VALUES (?, ?, ?)",
        )?;

        let tx = handle.unchecked_transaction()?;

        for (emoji, count) in &self.0 {
            stmt.execute([emoji.as_u64(), &count.text_count, &count.reaction_count])?;
        }

        tx.commit()?;
        Ok(())
    }
}

impl From<Vec<(EmojiId, EmojiStats)>> for EmojiUsage {
    fn from(vec: Vec<(EmojiId, EmojiStats)>) -> Self {
        Self(vec.into_iter().collect())
    }
}

impl LoadFromDatabase for EmojiUsage {
    type Item = (EmojiId, EmojiStats);

    fn load_from_database(handle: &Connection) -> anyhow::Result<Vec<Self::Item>>
    where
        Self: Sized,
    {
        let mut stmt = handle
            .prepare("SELECT emoji_id, text_count, reaction_count FROM emoji_usage")
            .context(here!())?;

        let result = stmt.query_and_then([], |row| -> anyhow::Result<(EmojiId, EmojiStats)> {
            Ok((
                EmojiId(row.get("emoji_id").context(here!())?),
                EmojiStats {
                    text_count: row.get("text_count").context(here!())?,
                    reaction_count: row.get("reaction_count").context(here!())?,
                },
            ))
        })?;

        result.collect()
    }
}

pub type ElementFormatter<'a, D> = Box<dyn Fn(&D, &Vec<String>) -> String + Send + Sync>;
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
    pub fn new() -> PaginatedList<'a, D> {
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

    pub async fn display(
        &'_ mut self,
        interaction: &'a Interaction,
        ctx: &'a Ctx,
    ) -> anyhow::Result<()> {
        let mut current_page: i32 = 1;

        if self.data.is_empty() {
            interaction
                .delete_original_interaction_response(&ctx.http)
                .await?;
            return Ok(());
        }

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

        let message = self
            .create_page(
                &data,
                current_page as usize,
                required_pages,
                interaction,
                ctx,
            )
            .await;

        let message = match message {
            Ok(msg) => msg,
            Err(err) => {
                error!("Error!!! {:#?}", err);
                return Err(anyhow!(err)).context(here!());
            }
        };

        if let Some(channel) = self.message_sender.take() {
            channel
                .send(message.clone())
                .map_err(|m| anyhow!("Could not send message: {}.", m.id))
                .context(here!())?;
        }

        if required_pages == 1 {
            return Ok(());
        }

        let mut message_recv;

        {
            let bot_data = ctx.data.read().await;
            message_recv = bot_data.get::<MessageSender>().unwrap().subscribe();
        }

        let token = self.token.take().unwrap_or_default();

        let page_turn_stream = message
            .await_component_interactions(&ctx.shard)
            .timeout(self.timeout);

        let page_turn_stream = match self.page_change_perm {
            PageChangePermission::Interactor => {
                page_turn_stream.author_id(interaction.member.as_ref().unwrap().user.id)
            }
            _ => page_turn_stream,
        };

        let mut page_turn_stream = Box::pin(page_turn_stream.await);

        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    break;
                }
                msg = message_recv.recv() => {
                    let id = match msg? {
                        MessageUpdate::Deleted(id) => id,
                        _ => continue
                    };

                    if id != message.id {
                        continue;
                    }

                    break;
                }
                page_turn = page_turn_stream.next() => {
                    let page_turn = match &page_turn {
                        Some(r) => r,
                        None => break,
                    };

                    let component_data = match &page_turn.data.as_ref().unwrap() {
                        InteractionData::MessageComponent(d) => d,
                        _ => continue,
                    };

                    match component_data.custom_id.as_str() {
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

                    page_turn.create_interaction_response(&ctx.http, |r| {
                        r.kind(InteractionResponseType::DeferredUpdateMessage)
                    }).await.context(here!())?;

                    self.create_page(
                        &data, current_page as usize,
                        required_pages,
                        interaction,
                        ctx,
                    )
                    .await?;
                }
            }
        }

        if self.delete_when_dropped {
            interaction
                .delete_original_interaction_response(&ctx.http)
                .await
                .context(here!())?;
        } else {
            interaction
                .edit_original_interaction_response(&ctx.http, |e| e.components(|c| c))
                .await
                .context(here!())?;
        }

        Ok(())
    }

    async fn create_page(
        &self,
        data: &FormattedData<'_, D>,
        page: usize,
        required_pages: usize,
        interaction: &Interaction,
        ctx: &Ctx,
    ) -> anyhow::Result<Message> {
        interaction
            .edit_original_interaction_response(&ctx.http, |r| {
                if required_pages > 1 {
                    r.components(|c| {
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
                            let birthdays_page = d
                                .iter()
                                .skip(((page - 1) as usize) * *items_per_page)
                                .take(*items_per_page);

                            for birthday in birthdays_page {
                                r.add_embed(func(birthday, &self.params));
                            }
                        }
                        _ => error!("Invalid layout and data format found!"),
                    }
                } else {
                    r.create_embed(|e| {
                        e.colour(Colour::new(6_282_735));

                        if let Some(title) = &self.title {
                            e.title(title);
                        }

                        match (&self.layout, data) {
                            (
                                PageLayout::Standard { items_per_page },
                                FormattedData::Standard(d),
                            ) => {
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
                                                chunk.iter().fold(
                                                    String::new(),
                                                    |mut acc, element| {
                                                        acc += match &self.format_func {
                                                            Some(func) => {
                                                                func(element, &self.params)
                                                            }
                                                            None => format!("{:?}", element),
                                                        }
                                                        .as_str();
                                                        acc
                                                    },
                                                ),
                                                true,
                                            )
                                        }),
                                );
                            }
                            _ => error!("Invalid layout and data format found!"),
                        }

                        match self.show_page_count {
                            ShowPageCount::Always => {
                                e.footer(|f| {
                                    f.text(format!("Page {} of {}", page, required_pages))
                                });
                            }
                            ShowPageCount::WhenSeveralPages if required_pages > 1 => {
                                e.footer(|f| {
                                    f.text(format!("Page {} of {}", page, required_pages))
                                });
                            }
                            _ => (),
                        }
                        e
                    });
                }

                r
            })
            .await
            .context(here!())
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

pub async fn should_fail<'a>(
    cfg: &'a Configuration,
    ctx: &'a Ctx,
    request: &'a Interaction,
    interaction: &'a RegisteredInteraction,
) -> Option<DispatchError> {
    if request.member.is_none() || request.channel_id.is_none() {
        return Some(DispatchError::OnlyForGuilds);
    }

    if cfg
        .blocked_users
        .contains(&request.member.as_ref().unwrap().user.id)
    {
        return Some(DispatchError::BlockedUser);
    }

    {
        if let Some(Channel::Guild(channel)) =
            request.channel_id.unwrap().to_channel_cached(&ctx).await
        {
            let guild_id = channel.guild_id;

            if cfg.blocked_guilds.contains(&guild_id) {
                return Some(DispatchError::BlockedGuild);
            }

            if let Some(guild) = guild_id.to_guild_cached(&ctx.cache).await {
                if cfg.blocked_users.contains(&guild.owner_id) {
                    return Some(DispatchError::BlockedGuild);
                }
            }
        }
    }

    if !cfg.allowed_channels.is_empty()
        && !cfg.allowed_channels.contains(&request.channel_id.unwrap())
    {
        return Some(DispatchError::BlockedChannel);
    }

    for check in interaction.options.checks.iter() {
        if !(check.function)(ctx, request, interaction) {
            return Some(DispatchError::CheckFailed(check.name, Reason::Unknown));
        }
    }

    None
}

#[derive(Debug, Clone)]
pub enum MessageUpdate {
    Sent(Message),
    Edited(Message),
    Deleted(MessageId),
}

pub async fn show_deferred_response(
    interaction: &Interaction,
    ctx: &Ctx,
    ephemeral: bool,
) -> anyhow::Result<()> {
    Interaction::create_interaction_response(interaction, &ctx.http, |r| {
        r.kind(InteractionResponseType::DeferredChannelMessageWithSource)
            .interaction_response_data(|d| {
                if ephemeral {
                    d.flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL);
                }

                d.content("Loading...")
            })
    })
    .await
    .context(here!())
}
