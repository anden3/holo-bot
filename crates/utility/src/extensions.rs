use serenity::model::{channel::Message, id::EmojiId};

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

pub trait MessageExt {
    fn get_emojis(&self) -> Vec<EmojiId>;
    fn is_only_emojis(&self) -> bool;
}

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
}
