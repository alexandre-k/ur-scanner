use std::sync::LazyLock;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    // pub app_env: String,
    pub app_url: String,
    pub app_port: u16,
    pub notification_url: String,
    pub ur_api_url: String,
    // pub otlp_endpoint: Option<String>,
    pub estates_config_path: String,
    pub supabase_service_role_key: String,
    pub supabase_url: String
}

pub static CONFIG: LazyLock<Config> = LazyLock::new(|| {
    dotenvy::from_filename(format!(".env")).ok();
    dotenvy::dotenv().ok();
    envy::from_env::<Config>().expect("Failed to load configuration from environment file")
});


#[derive(Debug, Clone, Deserialize, Hash, Eq, PartialEq)]
pub struct EstateTarget {
    pub name: String,
    pub shisya: String,
    pub danchi: String,
    pub shikibetu: String,
}

