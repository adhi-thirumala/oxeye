use crate::Context;
use oxeye_backend::helpers;
use oxeye_backend::helpers::{format_time_online, now};
use poise::CreateReply;
use poise::command;
use poise::serenity_prelude::{CreateEmbed, CreateEmbedFooter};

pub(crate) type Error = Box<dyn std::error::Error + Send + Sync>;

/// Autocomplete function for server names - suggests servers from current guild
async fn autocomplete_server_name(ctx: Context<'_>, partial: &str) -> Vec<String> {
    // Get guild_id from context
    let guild_id = match ctx.guild_id() {
        Some(id) => id.get(),
        None => return Vec::new(),
    };

    // Fetch servers for this guild
    let servers = ctx
        .data()
        .db
        .get_servers_by_guild(guild_id)
        .await
        .unwrap_or_default();

    // Filter server names by partial input (case-insensitive)
    servers
        .into_iter()
        .map(|s| s.name)
        .filter(|name| name.to_lowercase().contains(&partial.to_lowercase()))
        .take(25)
        .collect::<Vec<_>>()
}

/// Generate a one-time code to link a Minecraft server to this Discord server
#[command(slash_command, prefix_command, required_permissions = "ADMINISTRATOR")]
pub async fn connect(
    ctx: Context<'_>,
    #[description = "Minecraft Server Name"]
    #[autocomplete = "autocomplete_server_name"]
    name: String,
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
    #[description = "Minecraft Server Name"]
    #[autocomplete = "autocomplete_server_name"]
    name: String,
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

    // Get api_key_hash for building image URL
    let api_key_hash = data.db.get_api_key_hash_by_name(guild_id, &name).await?;

    // Build status text
    let status_text = if is_synced {
        format!("{} players online", server.players.len())
    } else {
        "awaiting sync".to_string()
    };

    let mut embed = CreateEmbed::default()
        .title(format!("{}", server.name))
        .color(0x5865F2);

    // Add status image only if synced and we have the api_key_hash
    if is_synced {
        if let Some(ref hash) = api_key_hash {
            let base_url = data.public_url.trim_end_matches('/');
            if !base_url.is_empty() {
                let image_url = format!("{}/status-image/{}.png?t={}", base_url, hash, now());
                tracing::info!("Generated status image URL: {}", image_url);
                embed = embed.image(image_url);
            }
        }
    }

    let embed = if !is_synced {
        embed.description(format!(
            "**Status:** {}\n\nThis server hasn't synced since the backend restarted. \
            Player data will update when someone joins/leaves or an admin runs `/oxeye sync` in-game.",
            status_text
        ))
    } else if server.players.is_empty() {
        embed.description("No players online")
    } else {
        let current_time = now();
        let player_list: String = server
            .players
            .iter()
            .map(|p| {
                let time_online = current_time - p.joined_at;
                let formatted_time = format_time_online(time_online);
                format!("{} ({})", p.player_name, formatted_time)
            })
            .collect::<Vec<_>>()
            .join(" | ");
        embed.description(format!("**{}** | {}", status_text, player_list))
    };
    ctx.send(CreateReply::default().embed(embed)).await?;
    Ok(())
}
