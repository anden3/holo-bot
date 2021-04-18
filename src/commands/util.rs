use serenity::framework::standard::{Configuration, DispatchError};

use super::{
    prelude::*,
    slash_types::{SlashCommandOptions, SlashGroupOptions},
};

pub async fn should_fail<'a>(
    cfg: &'a Configuration,
    ctx: &'a Context,
    inter: &'a Interaction,
    command: &'static SlashCommandOptions,
    group: &'static SlashGroupOptions,
) -> Option<DispatchError> {
    if (command.owner_privilege && group.owner_privilege)
        && cfg.owners.contains(&inter.member.user.id)
    {
        return None;
    }

    if cfg.blocked_users.contains(&inter.member.user.id) {
        return Some(DispatchError::BlockedUser);
    }

    {
        if let Some(Channel::Guild(channel)) = inter.channel_id.to_channel_cached(&ctx).await {
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

    if !cfg.allowed_channels.is_empty() && !cfg.allowed_channels.contains(&inter.channel_id) {
        return Some(DispatchError::BlockedChannel);
    }

    for check in group.checks.iter().chain(command.checks.iter()) {
        let res = (check.function)(ctx, inter, command).await;

        if let Result::Err(reason) = res {
            return Some(DispatchError::CheckFailed(check.name, reason));
        }
    }

    None
}
