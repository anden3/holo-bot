use serenity::framework::standard::{Configuration, DispatchError, Reason};

use utility::discord::RegisteredInteraction;

use crate::prelude::*;

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
