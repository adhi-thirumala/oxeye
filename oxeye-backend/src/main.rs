mod discord_commands;
use oxeye_backend::create_app;
use oxeye_db::Database;
use poise::{Framework, FrameworkOptions, serenity_prelude as serenity};
use tokio::net::TcpListener;

type Context<'a> = poise::Context<'a, crate::Data, crate::discord_commands::Error>;

pub(crate) struct Data {
  pub(crate) db: Database,
}

#[tokio::main]
async fn main() {
  // Initialize tracing for structured logging
  tracing_subscriber::fmt()
    .with_target(false)
    .compact()
    .init();
  tracing::info!("Starting Oxeye backend server...");
  // Load configuration from environment variables or use defaults
  let config = oxeye_backend::config::Config::from_env();
  tracing::info!(
    "Configuration: port={}, db_path={}, body_limit={}KB, timeout={}s",
    config.port,
    config.database_path,
    config.request_body_limit / 1024,
    config.request_timeout.as_secs()
  );
  let db = Database::open(&config.database_path).await.unwrap();
  let app = create_app(
    db.clone(),
    config.request_body_limit,
    config.request_timeout,
  );
  let addr = format!("0.0.0.0:{}", config.port);
  let listener = TcpListener::bind(&addr).await.unwrap();
  tracing::info!("Server listening on {}", addr);

  // send messages, send messages in threads, embed links, attach files, use external stickers and emoji, add reactions
  let intents =
    serenity::GatewayIntents::from_bits(412317173824).expect("Bad bytes lmfao in intent creation.");

  let framework = Framework::builder()
    .options(FrameworkOptions {
      commands: vec![
        discord_commands::connect(),
        discord_commands::list(),
        discord_commands::status(),
      ],
      pre_command: |ctx| {
        Box::pin(async move {
          tracing::info!(
            "Executing command '{}' by user '{}'",
            ctx.command().name,
            ctx.author().name
          );
        })
      },
      post_command: |ctx| {
        Box::pin(async move {
          tracing::info!(
            "Finished command '{}' by user '{}'",
            ctx.command().name,
            ctx.author().name
          );
        })
      },
      ..Default::default()
    })
    .setup(|ctx, _ready, framework| {
      Box::pin(async move {
        poise::builtins::register_globally(ctx, &framework.options().commands).await?;
        Ok(Data { db: db.clone() })
      })
    })
    .build();

  let mut client = serenity::ClientBuilder::new(&config.discord_token.unwrap(), intents)
    .framework(framework)
    .await
    .expect("Error creating Discord client");
  tokio::select! {
      result = axum::serve(listener, app) => {
          if let Err(e) = result {
              tracing::error!("Axum server error: {}", e);
          }
      }
      result = client.start() => {
          if let Err(e) = result {
              tracing::error!("Discord client error: {:?}", e);
          }
      }
  }
}
