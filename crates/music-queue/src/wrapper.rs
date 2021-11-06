use serenity::{client::Cache, http::Http, prelude::TypeMapKey};

use super::{prelude::*, Queue};

#[derive(Debug, Default)]
pub struct MusicData(pub HashMap<GuildId, Queue>);

impl MusicData {
    pub fn get_queue(&self, guild_id: &GuildId) -> Option<Queue> {
        self.get(guild_id).cloned()
    }

    pub fn is_guild_registered(&self, guild_id: &GuildId) -> bool {
        self.contains_key(guild_id)
    }

    pub fn register_guild(
        &mut self,
        manager: Arc<Songbird>,
        guild_id: &GuildId,
        discord_http: Arc<Http>,
        discord_cache: Arc<Cache>,
    ) {
        if self.contains_key(guild_id) {
            warn!("Attempted to register guild that was already registered!");
            return;
        }

        self.insert(
            *guild_id,
            Queue::new(manager, guild_id, discord_http, discord_cache),
        );
    }

    pub fn deregister_guild(&mut self, guild_id: &GuildId) {
        if self.remove(guild_id).is_none() {
            warn!("Attempted to deregister guild that wasn't registered!");
        }
    }
}

impl Deref for MusicData {
    type Target = HashMap<GuildId, Queue>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for MusicData {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl TypeMapKey for MusicData {
    type Value = MusicData;
}

impl IntoIterator for MusicData {
    type Item = (GuildId, Queue);
    type IntoIter = std::collections::hash_map::IntoIter<GuildId, Queue>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}
