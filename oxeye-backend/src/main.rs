mod discord_commands;
use oxeye_backend::{RateLimitConfig, create_app};
use oxeye_db::Database;
use poise::{Framework, FrameworkOptions, serenity_prelude as serenity};
use std::net::SocketAddr;
use tokio::net::TcpListener;

type Context<'a> = poise::Context<'a, crate::Data, crate::discord_commands::Error>;

pub(crate) struct Data {
    pub(crate) db: Database,
    pub(crate) public_url: String,
}

#[tokio::main]
async fn main() {
    // Initialize tracing for structured logging
    #[cfg(debug_assertions)]
    let log_level = tracing::Level::DEBUG;
    #[cfg(not(debug_assertions))]
    let log_level = tracing::Level::INFO;

    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .with_target(false)
        .compact()
        .init();
    tracing::info!("Starting Oxeye backend server...");
    // Load configuration from environment variables or use defaults
    let config = oxeye_backend::config::Config::from_env();
    tracing::info!(
        "Configuration: port={}, db_path={}, body_limit={}KB, timeout={}s, backend_url={}",
        config.port,
        config.database_path,
        config.request_body_limit / 1024,
        config.request_timeout.as_secs(),
        config.public_url
    );
    tracing::info!(
        "Rate limits: connect={}/min (burst {}), player={}/sec (burst {}), general={}/sec (burst {})",
        config.rate_limit_connect_per_min,
        config.rate_limit_connect_burst,
        config.rate_limit_player_per_sec,
        config.rate_limit_player_burst,
        config.rate_limit_general_per_sec,
        config.rate_limit_general_burst
    );
    let db = Database::open(&config.database_path).await.unwrap();
    let rate_limit = RateLimitConfig {
        connect_per_min: config.rate_limit_connect_per_min,
        connect_burst: config.rate_limit_connect_burst,
        player_per_sec: config.rate_limit_player_per_sec,
        player_burst: config.rate_limit_player_burst,
        general_per_sec: config.rate_limit_general_per_sec,
        general_burst: config.rate_limit_general_burst,
    };
    let app = create_app(
        db.clone(),
        config.request_body_limit,
        config.request_timeout,
        rate_limit,
    );
    let addr = format!("0.0.0.0:{}", config.port);
    let listener = TcpListener::bind(&addr).await.unwrap();
    tracing::info!("Server listening on {}", addr);

    // send messages, send messages in threads, embed links, attach files, use external stickers and emoji, add reactions
    let intents = serenity::GatewayIntents::default();

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
        .setup(move |ctx, _ready, framework| {
            let public_url = config.public_url.clone();
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(Data {
                    db: db.clone(),
                    public_url,
                })
            })
        })
        .build();

    let mut client = serenity::ClientBuilder::new(&config.discord_token.unwrap(), intents)
        .framework(framework)
        .await
        .expect("Error creating Discord client");
    tokio::select! {
        result = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()) => {
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
