mod config;
mod database;
mod bluebubbles;
mod ai_clients;
mod chat_agent;
mod orchestrator;
mod types;
mod commands;

use anyhow::Result;
use config::Config;
use orchestrator::BotOrchestrator;
use tracing::{info, error};
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("Starting AI iMessage Bot");

    // Load configuration
    let config = Config::load()?;
    info!("Configuration loaded successfully");

    // Create and start the bot orchestrator
    let mut orchestrator = BotOrchestrator::new(config).await?;
    
    info!("Bot orchestrator initialized, starting message polling...");
    
    // Start the main bot loop
    match orchestrator.run().await {
        Ok(_) => info!("Bot orchestrator finished successfully"),
        Err(e) => error!("Bot orchestrator failed: {}", e),
    }

    Ok(())
}