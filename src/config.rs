use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub openai_api_key: Option<String>,
    pub ollama_api: String,
    pub bluebubbles_api: String,
    pub bluebubbles_password: Option<String>,
    pub bot_trigger: String,
    pub ollama_model: String,
    pub database_url: String,
}

impl Config {
    pub fn load() -> Result<Self> {
        dotenv::dotenv().ok(); // Load .env file if it exists

        let config = Config {
            openai_api_key: env::var("OPENAI_API_KEY").ok(),
            ollama_api: env::var("OLLAMA_API").unwrap_or_else(|_| "http://localhost:11434".to_string()),
            bluebubbles_api: env::var("BLUEBUBBLES_API").unwrap_or_else(|_| "http://localhost:12345".to_string()),
            bluebubbles_password: env::var("BLUEBUBBLES_PASSWORD").ok(),
            bot_trigger: env::var("BOT_TRIGGER").unwrap_or_else(|_| "@ava".to_string()),
            ollama_model: env::var("OLLAMA_MODEL").unwrap_or_else(|_| "llama3.2".to_string()),
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:./bot.db".to_string()),
        };

        // Validate that we have at least one AI provider configured
        if config.openai_api_key.is_none() && config.ollama_api.is_empty() {
            return Err(anyhow::anyhow!("Must configure either OPENAI_API_KEY or OLLAMA_API"));
        }

        Ok(config)
    }

    pub fn triggers(&self) -> Vec<String> {
        vec![
            self.bot_trigger.to_lowercase(),
            "@character".to_string(),
            "@unhinge".to_string(),
        ]
    }
}