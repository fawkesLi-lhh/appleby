use std::{env, fs::read_to_string, path::Path, sync::LazyLock};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(alias = "anthropic_api_key")]
    pub openai_api_key: String,
    #[serde(alias = "anthropic_base_url")]
    pub openai_base_url: String,
    #[serde(alias = "anthropic_model")]
    pub openai_model: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            openai_api_key: env::var("OPENAI_API_KEY").unwrap_or_default(),
            openai_base_url: env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
            openai_model: env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string()),
        }
    }
}

impl Config {
    pub fn new() -> Self {
        // create dir if not exists
        if !Path::new(".appleby").exists() {
            std::fs::create_dir_all(".appleby").unwrap();
        }
        // create file if not exists
        if !Path::new(".appleby/config.toml").exists() {
            std::fs::write(
                ".appleby/config.toml",
                toml::to_string(&Config::default()).unwrap(),
            )
            .unwrap();
        }
        toml::from_str::<Config>(&read_to_string(".appleby/config.toml").unwrap()).unwrap()
    }
}

pub static CONFIG: LazyLock<Config> = LazyLock::new(Config::new);
