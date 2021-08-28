use std::{fmt::Display, sync::Arc};

use anyhow::Context;
use either::Either;
use itertools::{EitherOrBoth, Itertools};
use num::Integer;
use serenity::{
    builder::CreateEmbed,
    model::{channel::Message, id::ChannelId},
    CacheAndHttp,
};
use tokio::sync::Mutex;

use crate::here;

pub type EmbedFormatter<Arg> = Box<dyn Fn(&mut CreateEmbed, usize, &[Arg]) + Send + Sync>;

pub type SegmentLinkFn<Arg> = Box<dyn Fn(usize, &Message, &[Arg]) -> String + Send + Sync>;
pub type ElementFormatter<D, Arg> = Box<dyn Fn(&D, &[Arg]) -> String + Send + Sync>;

pub struct SegmentedMessage<D, Arg = String> {
    data: Vec<D>,
    position: SegmentDataPosition,
    order: DataOrder,

    colour: u32,

    index_fmt: Option<EmbedFormatter<Arg>>,
    segment_fmt: Option<EmbedFormatter<Arg>>,

    index_link_fn: SegmentLinkFn<Arg>,
    element_formatter: ElementFormatter<D, Arg>,

    args: Vec<Arg>,
}

impl<D, Arg> SegmentedMessage<D, Arg>
where
    D: Display,
    Arg: Clone,
{
    const MAX_DESCRIPTION_SIZE: usize = 4096;
    const MAX_FIELD_SIZE: usize = 1024;
    const MAX_TOTAL_BYTES: usize = 6000;

    const APPROX_LINK_LENGTH: usize = 128;
    const INVISIBLE_FIELD_NAME: &'static str = "\u{200b}";

    const LINKS_PER_INDEX_PAGE: usize = Self::MAX_DESCRIPTION_SIZE / Self::APPROX_LINK_LENGTH;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn data(&'_ mut self, data: Vec<D>) -> &'_ mut Self {
        self.data = data;
        self
    }

    pub fn position(&'_ mut self, position: SegmentDataPosition) -> &'_ mut Self {
        self.position = position;
        self
    }

    pub fn order(&'_ mut self, order: DataOrder) -> &'_ mut Self {
        self.order = order;
        self
    }

    pub fn colour<T: Into<u32>>(&'_ mut self, colour: T) -> &'_ mut Self {
        self.colour = colour.into();
        self
    }

    pub fn index_format(&'_ mut self, format: EmbedFormatter<Arg>) -> &'_ mut Self {
        self.index_fmt = Some(format);
        self
    }

    pub fn segment_format(&'_ mut self, format: EmbedFormatter<Arg>) -> &'_ mut Self {
        self.segment_fmt = Some(format);
        self
    }

    pub fn format(&'_ mut self, format: ElementFormatter<D, Arg>) -> &'_ mut Self {
        self.element_formatter = format;
        self
    }

    pub fn link_format(&'_ mut self, format: SegmentLinkFn<Arg>) -> &'_ mut Self {
        self.index_link_fn = format;
        self
    }

    pub fn args(&'_ mut self, args: &[Arg]) -> &'_ mut Self {
        self.args = args.to_vec();
        self
    }

    pub async fn create(
        &mut self,
        ctx: &Arc<CacheAndHttp>,
        ch: Arc<Mutex<ChannelId>>,
    ) -> anyhow::Result<()> {
        let data_iter = match self.order {
            DataOrder::Normal => Either::Left(self.data.iter()),
            DataOrder::Reverse => Either::Right(self.data.iter().rev()),
        };

        let limit = match self.position {
            SegmentDataPosition::Description => Self::MAX_DESCRIPTION_SIZE,
            SegmentDataPosition::Fields => Self::MAX_FIELD_SIZE,
        };

        let chunks = data_iter
            .map(|d| (self.element_formatter)(d, &self.args))
            .coalesce(|a, b| {
                if a.len() + b.len() <= limit {
                    Ok(a + &b)
                } else {
                    Err((a, b))
                }
            })
            .collect::<Vec<String>>();

        let log_ch = ch.lock().await;

        let max_chunks_per_message = match &self.position {
            SegmentDataPosition::Description => 1,
            SegmentDataPosition::Fields => Self::MAX_TOTAL_BYTES / Self::MAX_FIELD_SIZE,
        };

        if chunks.len() <= max_chunks_per_message {
            self.create_segment(ctx, *log_ch, 0, &chunks, &self.index_fmt)
                .await?;

            return Ok(());
        }

        let approx_segments_needed = chunks.len().div_ceil(&max_chunks_per_message);
        let index_pages_needed = approx_segments_needed.div_ceil(&Self::LINKS_PER_INDEX_PAGE);

        let mut index_pages = Vec::with_capacity(index_pages_needed);

        for i in 0..index_pages_needed {
            index_pages.push(
                log_ch
                    .send_message(&ctx.http, |m| {
                        m.content(format!("Reserved index page {}", i + 1))
                    })
                    .await?,
            );
        }

        let mut log_message_links = Vec::with_capacity(approx_segments_needed);

        for (i, chunk) in chunks.chunks(max_chunks_per_message).enumerate() {
            log_message_links.push(
                self.create_segment(ctx, *log_ch, i, chunk, &self.segment_fmt)
                    .await?,
            );
        }

        drop(log_ch);

        let indices = log_message_links
            .into_iter()
            .enumerate()
            .map(|(i, msg)| (self.index_link_fn)(i, &msg, &self.args))
            .coalesce(|a, b| {
                if a.len() + b.len() <= Self::MAX_DESCRIPTION_SIZE {
                    Ok(a + &b)
                } else {
                    Err((a, b))
                }
            })
            .collect::<Vec<String>>();

        assert!(indices.len() <= index_pages.len());

        let indices = index_pages
            .into_iter()
            .zip_longest(indices.into_iter())
            .enumerate()
            .collect::<Vec<(_, _)>>();

        let prev_position = self.position;
        self.position = SegmentDataPosition::Description;

        for (i, index) in indices {
            match index {
                // If all the links fit in previous pages, delete this one.
                EitherOrBoth::Left(msg) => msg.delete(&ctx).await.context(here!()),
                EitherOrBoth::Right(_) => unreachable!(),
                EitherOrBoth::Both(mut msg, link) => self
                    .edit_segment(ctx, &mut msg, i, &[link], &self.index_fmt)
                    .await
                    .context(here!())
                    .map(|_| ()),
            }?;
        }

        self.position = prev_position;

        Ok(())
    }

    #[allow(clippy::manual_async_fn)]
    #[fix_hidden_lifetime_bug]
    async fn create_segment(
        &self,
        ctx: &Arc<CacheAndHttp>,
        ch: ChannelId,
        i: usize,
        data: &[String],
        formatter: &Option<EmbedFormatter<Arg>>,
    ) -> anyhow::Result<Message> {
        ch.send_message(&ctx.http, |m| {
            m.embed(|e| self.format_segment_embed(e, i, data, formatter))
        })
        .await
        .context(here!())
    }

    async fn edit_segment(
        &self,
        ctx: &Arc<CacheAndHttp>,
        msg: &mut Message,
        i: usize,
        data: &[String],
        formatter: &Option<EmbedFormatter<Arg>>,
    ) -> anyhow::Result<()> {
        msg.edit(ctx, |m| {
            m.embed(|e| self.format_segment_embed(e, i, data, formatter))
        })
        .await
        .context(here!())
    }

    fn format_segment_embed<'a>(
        &self,
        embed: &'a mut CreateEmbed,
        i: usize,
        data: &[String],
        formatter: &Option<EmbedFormatter<Arg>>,
    ) -> &'a mut CreateEmbed {
        if let Some(fmt) = formatter {
            fmt(embed, i, &self.args);
        }

        embed.colour(self.colour);

        match &self.position {
            SegmentDataPosition::Description => embed.description(data.join("")),
            SegmentDataPosition::Fields => {
                embed.fields(data.iter().map(|c| (Self::INVISIBLE_FIELD_NAME, c, false)))
            }
        }
    }
}

impl<D: Display, Arg> Default for SegmentedMessage<D, Arg> {
    fn default() -> Self {
        Self {
            data: Vec::new(),
            args: Vec::new(),

            colour: 6_282_735,

            order: DataOrder::Normal,
            position: SegmentDataPosition::Fields,

            index_fmt: None,
            segment_fmt: None,

            index_link_fn: Box::new(|i, msg, _| format!("[Segment {}]({})\n", i + 1, msg.link())),
            element_formatter: Box::new(|d, _| d.to_string()),
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum SegmentDataPosition {
    Description,
    Fields,
}

#[derive(Debug, Copy, Clone)]
pub enum DataOrder {
    Normal,
    Reverse,
}
