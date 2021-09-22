use super::{prelude::*, BufferedQueue};

#[derive(Debug, Default)]
pub struct MusicData(pub HashMap<GuildId, BufferedQueue>);

impl MusicData {
    pub fn get_queue(&self, guild_id: &GuildId) -> Option<BufferedQueue> {
        self.get(guild_id).cloned()
    }

    pub fn is_guild_registered(&self, guild_id: &GuildId) -> bool {
        self.contains_key(guild_id)
    }

    pub fn register_guild(&mut self, manager: Arc<Songbird>, guild_id: &GuildId) {
        if self.contains_key(guild_id) {
            return;
        }

        self.insert(*guild_id, BufferedQueue::new(manager, guild_id));
    }

    pub fn deregister_guild(&mut self, guild_id: &GuildId) {
        if self.remove(guild_id).is_none() {
            warn!("Attempted to deregister guild that wasn't registered!");
        }
    }
}
