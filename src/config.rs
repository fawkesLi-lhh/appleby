use std::{fs::read_to_string, sync::LazyLock};

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub anthropic_api_key: String,
    pub anthropic_base_url: String,
    pub anthropic_model: String,
}

impl Config {
    pub fn new() -> Self {
        let config =
            toml::from_str::<Config>(&read_to_string("conf/config.toml").unwrap()).unwrap();
        config
    }
}

pub static CONFIG: LazyLock<Config> = LazyLock::new(|| Config::new());
