use anyhow::Result;
use regex::Regex;
use tracing::{debug, info};
use crate::types::ChatConfig;
use crate::ai_clients::AIClients;
use crate::database::Database;
use chrono::Utc;

#[derive(Debug, Clone)]
pub enum Command {
    Character { description: String },
    Unhinge { enabled: bool },
}

pub struct CommandParser {
    character_regex: Regex,
    unhinge_regex: Regex,
}

impl CommandParser {
    pub fn new() -> Result<Self> {
        Ok(Self {
            character_regex: Regex::new(r"@character\s+(.+)")?,
            unhinge_regex: Regex::new(r"@unhinge\s+(.+)")?,
        })
    }

    pub fn parse_command(&self, text: &str) -> Option<Command> {
        let text = text.trim();

        // Check for character command
        if let Some(captures) = self.character_regex.captures(text) {
            let description = captures.get(1)?.as_str().trim().to_string();
            if !description.is_empty() {
                debug!("Parsed character command: {}", description);
                return Some(Command::Character { description });
            }
        }

        // Check for unhinge command
        if let Some(captures) = self.unhinge_regex.captures(text) {
            let value = captures.get(1)?.as_str().trim().to_lowercase();
            let enabled = value == "true" || value == "1" || value == "on" || value == "yes";
            debug!("Parsed unhinge command: {}", enabled);
            return Some(Command::Unhinge { enabled });
        }

        None
    }
}

pub struct CommandHandler {
    parser: CommandParser,
    ai_clients: AIClients,
    database: Database,
}

impl CommandHandler {
    pub fn new(ai_clients: AIClients, database: Database) -> Result<Self> {
        Ok(Self {
            parser: CommandParser::new()?,
            ai_clients,
            database,
        })
    }

    pub async fn handle_command(
        &self,
        chat_guid: &str,
        text: &str,
        config: &mut ChatConfig,
    ) -> Result<Option<String>> {
        if let Some(command) = self.parser.parse_command(text) {
            match command {
                Command::Character { description } => {
                    self.handle_character_command(chat_guid, &description, config).await
                }
                Command::Unhinge { enabled } => {
                    self.handle_unhinge_command(chat_guid, enabled, config).await
                }
            }
        } else {
            Ok(None)
        }
    }

    async fn handle_character_command(
        &self,
        chat_guid: &str,
        description: &str,
        config: &mut ChatConfig,
    ) -> Result<Option<String>> {
        info!("Handling character command for chat {}: {}", chat_guid, description);

        // Generate character prompt using AI
        let character_prompt = match self.ai_clients.generate_character_prompt(description).await {
            Ok(prompt) => prompt,
            Err(e) => {
                return Ok(Some(format!(
                    "❌ Failed to generate character prompt: {} — MyAI",
                    e
                )));
            }
        };

        info!("Generated character prompt: {}", &character_prompt[..100.min(character_prompt.len())]);

        // Update chat config
        config.character_prompt = Some(character_prompt);
        config.updated_at = Utc::now();

        // Save to database
        if let Err(e) = self.database.save_chat_config(config).await {
            return Ok(Some(format!(
                "❌ Failed to save character config: {} — MyAI",
                e
            )));
        }

        // Clear chat context since we're switching characters
        // This will be handled by the caller

        Ok(Some(format!(
            "✅ Character updated! I'm now: {} — MyAI",
            description
        )))
    }

    async fn handle_unhinge_command(
        &self,
        chat_guid: &str,
        enabled: bool,
        config: &mut ChatConfig,
    ) -> Result<Option<String>> {
        info!("Handling unhinge command for chat {}: {}", chat_guid, enabled);

        // Update chat config
        config.use_ollama = enabled;
        config.updated_at = Utc::now();

        // Save to database
        if let Err(e) = self.database.save_chat_config(config).await {
            return Ok(Some(format!(
                "❌ Failed to save unhinge config: {} — MyAI",
                e
            )));
        }

        let status = if enabled { "enabled" } else { "disabled" };
        Ok(Some(format!(
            "✅ Unhinge mode {} — MyAI",
            status
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_character_command_parsing() {
        let parser = CommandParser::new().unwrap();

        let cmd = parser.parse_command("@character a witty robot");
        assert!(matches!(cmd, Some(Command::Character { .. })));

        if let Some(Command::Character { description }) = cmd {
            assert_eq!(description, "a witty robot");
        }
    }

    #[test]
    fn test_unhinge_command_parsing() {
        let parser = CommandParser::new().unwrap();

        let cmd = parser.parse_command("@unhinge true");
        assert!(matches!(cmd, Some(Command::Unhinge { enabled: true })));

        let cmd = parser.parse_command("@unhinge false");
        assert!(matches!(cmd, Some(Command::Unhinge { enabled: false })));

        let cmd = parser.parse_command("@unhinge on");
        assert!(matches!(cmd, Some(Command::Unhinge { enabled: true })));
    }

    #[test]
    fn test_no_command() {
        let parser = CommandParser::new().unwrap();

        let cmd = parser.parse_command("Just a regular message");
        assert!(cmd.is_none());

        let cmd = parser.parse_command("@ava hello there");
        assert!(cmd.is_none());
    }
}