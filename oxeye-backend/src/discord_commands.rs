use crate::Context;
use oxeye_backend::helpers;
use oxeye_backend::helpers::now;
use poise::command;
use poise::serenity_prelude::{CreateEmbed, CreateEmbedFooter};
use poise::CreateReply;

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
  data
    .db
    .create_pending_link(code.clone(), guild_id, name, now())
    .await?;
  ctx
    .send(
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

#[command(slash_command, prefix_command, required_permissions = "ADMINISTRATOR")]
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
  let server = data.db.get_server_with_players(guild_id, name).await?;
  let embed = CreateEmbed::default()
    .title(format!("Status: {}", server.name))
    .color(0x5865F2);
  let embed = if server.players.is_empty() {
    embed
      .description("No players online")
      .field("Players", "0", true)
  } else {
    let player_list: String = server
      .players
      .iter()
      .map(|p| format!("- {}", p))
      .collect::<Vec<_>>()
      .join("\n");
    embed
      .field("Online", format!("{}", server.players.len()), true)
      .field("Players", player_list, false)
  };
  ctx.send(CreateReply::default().embed(embed)).await?;
  Ok(())
}
