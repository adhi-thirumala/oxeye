use crate::Context;
use oxeye_backend::helpers;
use oxeye_backend::helpers::{format_time_online, now};
use poise::CreateReply;
use poise::command;
use poise::serenity_prelude::{CreateEmbed, CreateEmbedFooter};

pub(crate) type Error = Box<dyn std::error::Error + Send + Sync>;

/// Generate a one-time code to link a Minecraft server to this Discord server
#[command(slash_command, prefix_command, required_permissions = "ADMINISTRATOR")]
pub async fn connect(
    ctx: Context<'_>,
    #[description = "Minecraft Server Name"] name: String,
) -> Result<(), Error> {
    let data = ctx.data();
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get();
    let code = helpers::generate_code();
    data.db
        .create_pending_link(code.clone(), guild_id, name, now())
        .await?;
    ctx.send(
        CreateReply::default().embed(
            CreateEmbed::default()
                .title("Link Your Server")
                .description("Run this command in your Minecraft server:")
                .field("Command", format!("`/oxeye connect {}`", code), false)
                .field("Expires", "10 minutes", true)
                .color(0x5865F2)
                .footer(CreateEmbedFooter::new(
                    "Only server admins can run this command",
                )),
        ),
    )
    .await?;
    Ok(())
}

/// List all Minecraft servers linked to this Discord server
#[command(slash_command, prefix_command)]
pub async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let data = ctx.data();
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get();
    let servers = data.db.get_servers_by_guild(guild_id).await?;
    let embed = CreateEmbed::default()
        .title("Linked Minecraft Servers")
        .color(0x5865F2);
    let embed = if servers.is_empty() {
        embed.description("No servers linked yet.")
    } else {
        let list: String = servers
            .iter()
            .map(|s| format!("- {}", s.name))
            .collect::<Vec<_>>()
            .join("\n");
        embed.description(list)
    };
    ctx.send(CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// Show online players for a linked Minecraft server
#[command(slash_command, prefix_command)]
pub async fn status(
    ctx: Context<'_>,
    #[description = "Minecraft Server Name"] name: String,
) -> Result<(), Error> {
    let data = ctx.data();
    let guild_id = ctx
        .guild_id()
        .ok_or("This command can only be used in a server")?
        .get();
    let server = data
        .db
        .get_server_with_players(guild_id, name.clone())
        .await?;
    let is_synced = data
        .db
        .is_server_synced_by_name(guild_id, &name)
        .await
        .unwrap_or(false);

    // Show sync status indicator
    let (status_icon, status_text) = if is_synced {
        ("🟢", format!("{} players online", server.players.len()))
    } else {
        ("⏳", "awaiting sync".to_string())
    };

    let embed = CreateEmbed::default()
        .title(format!("{} {}", status_icon, server.name))
        .color(0x5865F2);

    let embed = if !is_synced {
        embed.description(format!(
            "**Status:** {}\n\nThis server hasn't synced since the backend restarted. \
            Player data will update when someone joins/leaves or an admin runs `/oxeye sync` in-game.",
            status_text
        ))
    } else if server.players.is_empty() {
        embed
            .description("No players online")
            .field("Players", "0", true)
    } else {
        let current_time = now();
        let player_list: String = server
            .players
            .iter()
            .map(|p| {
                let time_online = current_time - p.joined_at;
                let formatted_time = format_time_online(time_online);
                format!("- {} (Joined {} ago)", p.player_name, formatted_time)
            })
            .collect::<Vec<_>>()
            .join("\n");
        embed
            .field("Online", format!("{}", server.players.len()), true)
            .field("Players", player_list, false)
    };
    ctx.send(CreateReply::default().embed(embed)).await?;
    Ok(())
}
