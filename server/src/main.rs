use std::error::Error;

use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use zeroclaw_server::{app, config::Config, db::create_pool};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    init_tracing();

    let config = Config::from_env();
    let pool = create_pool(&config.database_url).await?;
    let listener = TcpListener::bind(config.bind_address).await?;

    info!(seed_on_startup = config.seed_on_startup, "database pool established");
    info!(address = %config.bind_address, "server listening");

    let _pool = pool;

    axum::serve(listener, app()).await?;

    Ok(())
}

fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("zeroclaw_server=debug,tower_http=info"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}
