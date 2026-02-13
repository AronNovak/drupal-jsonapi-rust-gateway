use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub drupal: DrupalConfig,
    pub cache: CacheConfig,
    pub pagination: PaginationConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub base_url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    pub pool_size: u32,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct DrupalConfig {
    pub backend_url: String,
    pub jsonapi_prefix: String,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct CacheConfig {
    pub collection_max_age: u64,
    pub individual_max_age: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PaginationConfig {
    pub default_limit: u64,
    pub max_limit: u64,
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let content = std::fs::read_to_string(path)?;
        let config: AppConfig = toml::from_str(&content)?;
        Ok(config)
    }
}
