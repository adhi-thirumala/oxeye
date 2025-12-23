use crate::Context;
use oxeye_backend::helpers;
use oxeye_backend::helpers::now;
use poise::command;

pub(crate) type Error = Box<dyn std::error::Error + Send + Sync>;

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
    let response = format!(
        "Your server link code is: `{}`\n
        Use this code in the Oxeye Minecraft mod to link your server to this Discord server. \n
        This code will expire in 10 minutes. \n
        Note that only administrators can use this command on the Minecraft server.",
        code
    );
    data
        .db
        .create_pending_link(code, guild_id, name, now())
        .await?;
    ctx.say(response).await?;
    Ok(())
}

#[command(slash_command, prefix_command, required_permissions = "ADMINISTRATOR")]
pub async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let data = ctx.data();
    let guild_id = ctx.guild_id().ok_or("This command can only be used in a server")?.get();
    let servers = data.db.get_servers_by_guild(guild_id).await?;
    if servers.is_empty() {
        ctx.say("No Minecraft servers are linked to this Discord server.").await?;
    } else {
        let mut response = String::from("Linked Minecraft Servers:\n");
        for server in servers {
            response.push_str(&format!("- {}\n", server.name));
        }
        ctx.say(response).await?;
    }
    Ok(())
}

#[command(slash_command, prefix_command, required_permissions = "ADMINISTRATOR")]
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
        .db.get_server_with_players(guild_id, name)
        .await?;
    if server.players.is_empty() {
        ctx.say(format!(
            "No players are currently connected to the server '{}'.",
            server.name
        ))
            .await?;
    } else {
        let mut response = format!(
            "Players currently connected to '{}':\n",
            server.name
        );
        for player in server.players {
            response.push_str(&format!("- {}\n", player));
        }
        ctx.say(response).await?;
    }
    Ok(())
}
