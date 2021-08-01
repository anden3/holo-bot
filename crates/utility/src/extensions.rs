use std::sync::Arc;

use anyhow::{anyhow, bail, Context};
use serenity::{
    async_trait,
    builder::CreateEmbed,
    model::{channel::Message, id::EmojiId},
    CacheAndHttp,
};
use tracing::warn;

use crate::here;

pub trait VecExt<T> {
    fn sort_unstable_by_key_ref<F, K>(&mut self, key: F)
    where
        F: Fn(&T) -> &K,
        K: ?Sized + Ord;
}

impl<T> VecExt<T> for Vec<T> {
    fn sort_unstable_by_key_ref<F, K>(&mut self, key: F)
    where
        F: Fn(&T) -> &K,
        K: ?Sized + Ord,
    {
        self.sort_unstable_by(|x, y| key(x).cmp(key(y)));
    }
}

#[async_trait]
pub trait MessageExt {
    fn get_emojis(&self) -> Vec<EmojiId>;
    fn is_only_emojis(&self) -> bool;
    fn get_embed_rows(&self) -> anyhow::Result<Vec<&str>>;

    async fn add_embed_row(
        &mut self,
        ctx: &Arc<CacheAndHttp>,
        embed: &CreateEmbed,
        text: String,
    ) -> anyhow::Result<EmbedRowAddition>;

    async fn edit_embed_row(
        &mut self,
        ctx: &Arc<CacheAndHttp>,
        embed: &CreateEmbed,
        row: usize,
        text: String,
    ) -> anyhow::Result<EmbedRowEdit>;

    async fn remove_embed_row(
        &mut self,
        ctx: &Arc<CacheAndHttp>,
        embed: &CreateEmbed,
        row: usize,
    ) -> anyhow::Result<EmbedRowRemoval>;
}

#[async_trait]
impl MessageExt for Message {
    fn get_emojis(&self) -> Vec<EmojiId> {
        let emoji_rgx: &regex::Regex = crate::regex!(r#"<a?:(\w+):(\d+)>"#);

        emoji_rgx
            .captures_iter(&self.content)
            .map(|caps| EmojiId(caps[2].parse().unwrap()))
            .collect()
    }

    fn is_only_emojis(&self) -> bool {
        let emoji_rgx: &regex::Regex = crate::regex!(r#"<a?:(\w+):(\d+)>"#);
        emoji_rgx.replace_all(&self.content, "").trim().is_empty()
    }

    fn get_embed_rows(&self) -> anyhow::Result<Vec<&str>> {
        Ok(self
            .embeds
            .first()
            .ok_or_else(|| anyhow!("Message doesn't contain an embed!"))?
            .description
            .as_ref()
            .ok_or_else(|| anyhow!("Message has no description!"))?
            .lines()
            .collect::<Vec<_>>())
    }

    async fn add_embed_row(
        &mut self,
        ctx: &Arc<CacheAndHttp>,
        embed: &CreateEmbed,
        text: String,
    ) -> anyhow::Result<EmbedRowAddition> {
        let new_text = match self.embeds.first().and_then(|e| e.description.clone()) {
            Some(t) => format!("{}\n{}", t, text),
            None => text,
        };

        let size = new_text.len();

        self.edit(&ctx, |e| {
            e.set_embed(embed.clone().description(&new_text).to_owned())
        })
        .await
        .context(here!())?;

        Ok(EmbedRowAddition { size })
    }

    async fn edit_embed_row(
        &mut self,
        ctx: &Arc<CacheAndHttp>,
        embed: &CreateEmbed,
        row: usize,
        text: String,
    ) -> anyhow::Result<EmbedRowEdit> {
        let mut lines = self.get_embed_rows().context(here!())?;

        // Make sure the edit doesn't overflow the max message size.
        let max_line_length =
            4096 - lines.iter().fold(0, |sum, l| sum + l.len()) + lines[row].len();

        let text = if text.len() > max_line_length {
            warn!("Edit makes embed description too large, truncating to valid size...");
            &text[0..max_line_length]
        } else {
            &text
        };

        lines[row] = text;

        let new_text = lines.join("\n");
        let size = new_text.len();

        self.edit(&ctx, |e| {
            e.set_embed(embed.clone().description(&new_text).to_owned())
        })
        .await
        .context(here!())?;

        Ok(EmbedRowEdit { size })
    }

    async fn remove_embed_row(
        &mut self,
        ctx: &Arc<CacheAndHttp>,
        embed: &CreateEmbed,
        row: usize,
    ) -> anyhow::Result<EmbedRowRemoval> {
        let mut lines = self.get_embed_rows().context(here!())?;

        if row >= lines.len() {
            bail!("Row index out of bounds!");
        }

        if lines.len() == 1 {
            self.delete(&ctx).await.context(here!())?;

            Ok(EmbedRowRemoval {
                msg_deleted: true,
                ..Default::default()
            })
        } else {
            lines.remove(row);

            let last_row = lines.len() - 1;
            let new_text = lines.join("\n");
            let size = new_text.len();

            self.edit(&ctx, |e| {
                e.set_embed(embed.clone().description(&new_text).to_owned())
            })
            .await
            .context(here!())?;

            Ok(EmbedRowRemoval {
                msg_deleted: false,
                last_row,
                size,
            })
        }
    }
}

#[derive(Default)]
pub struct EmbedRowAddition {
    pub size: usize,
}

#[derive(Default)]
pub struct EmbedRowEdit {
    pub size: usize,
}

#[derive(Default)]
pub struct EmbedRowRemoval {
    pub msg_deleted: bool,
    pub last_row: usize,
    pub size: usize,
}
