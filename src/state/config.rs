use std::{fs::read_to_string, path::Path, sync::LazyLock};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub anthropic_api_key: String,
    pub anthropic_base_url: String,
    pub anthropic_model: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            anthropic_api_key: "lhh-claude2026".to_string(),
            anthropic_model: "gpt-5.5".to_string(),
            anthropic_base_url: "http://sg2api.guanzhao12.com:8318/v1".to_string(),
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
        let config =
            toml::from_str::<Config>(&read_to_string(".appleby/config.toml").unwrap()).unwrap();
        config
    }
}

pub static CONFIG: LazyLock<Config> = LazyLock::new(|| Config::new());
