use super::prelude::*;

#[derive(Debug, Default)]
pub struct Permissions(pub u64);

impl Permissions {
    pub fn from_str(s: &str) -> Option<Self> {
        Some(Permissions(match s.to_uppercase().as_str() {
            "PRESET_GENERAL" => 0b0000_0110_0011_0111_1101_1100_0100_0001,
            "PRESET_TEXT" => 0b0000_0000_0000_0111_1111_1100_0100_0000,
            "PRESET_VOICE" => 0b0000_0011_1111_0000_0000_0000_0000_0000,
            "CREATE_INVITE" => 0b0000_0000_0000_0000_0000_0000_0000_0001,
            "KICK_MEMBERS" => 0b0000_0000_0000_0000_0000_0000_0000_0010,
            "BAN_MEMBERS" => 0b0000_0000_0000_0000_0000_0000_0000_0100,
            "ADMINISTRATOR" => 0b0000_0000_0000_0000_0000_0000_0000_1000,
            "MANAGE_CHANNELS" => 0b0000_0000_0000_0000_0000_0000_0001_0000,
            "MANAGE_GUILD" => 0b0000_0000_0000_0000_0000_0000_0010_0000,
            "ADD_REACTIONS" => 0b0000_0000_0000_0000_0000_0000_0100_0000,
            "VIEW_AUDIT_LOG" => 0b0000_0000_0000_0000_0000_0000_1000_0000,
            "PRIORITY_SPEAKER" => 0b0000_0000_0000_0000_0000_0001_0000_0000,
            "READ_MESSAGES" => 0b0000_0000_0000_0000_0000_0100_0000_0000,
            "SEND_MESSAGES" => 0b0000_0000_0000_0000_0000_1000_0000_0000,
            "SEND_TTS_MESSAGES" => 0b0000_0000_0000_0000_0001_0000_0000_0000,
            "MANAGE_MESSAGES" => 0b0000_0000_0000_0000_0010_0000_0000_0000,
            "EMBED_LINKS" => 0b0000_0000_0000_0000_0100_0000_0000_0000,
            "ATTACH_FILES" => 0b0000_0000_0000_0000_1000_0000_0000_0000,
            "READ_MESSAGE_HISTORY" => 0b0000_0000_0000_0001_0000_0000_0000_0000,
            "MENTION_EVERYONE" => 0b0000_0000_0000_0010_0000_0000_0000_0000,
            "USE_EXTERNAL_EMOJIS" => 0b0000_0000_0000_0100_0000_0000_0000_0000,
            "CONNECT" => 0b0000_0000_0001_0000_0000_0000_0000_0000,
            "SPEAK" => 0b0000_0000_0010_0000_0000_0000_0000_0000,
            "MUTE_MEMBERS" => 0b0000_0000_0100_0000_0000_0000_0000_0000,
            "DEAFEN_MEMBERS" => 0b0000_0000_1000_0000_0000_0000_0000_0000,
            "MOVE_MEMBERS" => 0b0000_0001_0000_0000_0000_0000_0000_0000,
            "USE_VAD" => 0b0000_0010_0000_0000_0000_0000_0000_0000,
            "CHANGE_NICKNAME" => 0b0000_0100_0000_0000_0000_0000_0000_0000,
            "MANAGE_NICKNAMES" => 0b0000_1000_0000_0000_0000_0000_0000_0000,
            "MANAGE_ROLES" => 0b0001_0000_0000_0000_0000_0000_0000_0000,
            "MANAGE_WEBHOOKS" => 0b0010_0000_0000_0000_0000_0000_0000_0000,
            "MANAGE_EMOJIS" => 0b0100_0000_0000_0000_0000_0000_0000_0000,
            _ => return None,
        }))
    }
}

impl ToTokens for Permissions {
    fn to_tokens(&self, stream: &mut TokenStream2) {
        let bits = self.0;
        let path = quote!(serenity::model::permissions::Permissions);

        stream.extend(quote! {
            #path { bits: #bits }
        });
    }
}
