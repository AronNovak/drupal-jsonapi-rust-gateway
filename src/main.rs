mod config;
mod document;
mod entity_loader;
mod error;
mod field_loader;
mod handlers;
mod include_resolver;
mod php_unserialize;
mod proxy;
mod query;
mod routes;
mod schema;
mod serializer;
mod types;

use config::AppConfig;
use handlers::AppState;
use sqlx::mysql::MySqlPoolOptions;
use std::path::Path;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::compression::CompressionLayer;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = AppConfig::load(Path::new("config.toml"))?;
    tracing::info!("Loaded config: {}:{}", config.server.host, config.server.port);

    let pool = MySqlPoolOptions::new()
        .max_connections(config.database.pool_size)
        .connect(&config.database.url)
        .await?;
    tracing::info!("Connected to database");

    let resource_type_map = schema::discover_schema(&pool).await?;
    let config_entity_uuids = schema::load_all_config_entity_uuids(&pool).await?;

    let state = Arc::new(AppState {
        pool,
        resource_type_map,
        config: config.clone(),
        config_entity_uuids,
    });

    let app = routes::build_router(state)
        .layer(CorsLayer::permissive())
        .layer(CompressionLayer::new());

    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Listening on {}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}
