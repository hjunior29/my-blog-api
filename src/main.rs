use std::process::ExitCode;

use my_blog_api::{config::Config, database, router_with_config, shutdown::Shutdown};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .json()
        .init();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "application stopped");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env()?;
    let shutdown = Shutdown::new()?;
    let listener = tokio::net::TcpListener::bind(config.bind_address).await?;
    let pool = database::connect(&config.database_url, config.database_max_connections).await?;
    tracing::info!(address = %listener.local_addr()?, "HTTP server listening");
    let make_service = router_with_config(pool.clone(), config)
        .into_make_service_with_connect_info::<std::net::SocketAddr>();
    let result = axum::serve(listener, make_service)
        .with_graceful_shutdown(shutdown.wait())
        .await;
    pool.close().await;
    result?;
    Ok(())
}
