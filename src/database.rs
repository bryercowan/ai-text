use anyhow::{Result, Context};
use chrono::Utc;
use sqlx::{Row, SqlitePool, ConnectOptions};
use std::{fs, str::FromStr};
use crate::types::{ChatConfig, Message, MessageRole};

#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn new(database_url: &str) -> Result<Self> {
        // Ensure database directory exists for SQLite file databases
        if database_url.starts_with("sqlite:") {
            let db_path = database_url.strip_prefix("sqlite:").unwrap_or(database_url);
            if let Some(parent) = std::path::Path::new(db_path).parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)
                        .context("Failed to create database directory")?;
                }
            }
        }

        // Use connect options to create the database file if it doesn't exist
        let pool = SqlitePool::connect_with(
            sqlx::sqlite::SqliteConnectOptions::from_str(database_url)?
                .create_if_missing(true)
        )
        .await
        .context("Failed to connect to database")?;
        
        let db = Self { pool };
        db.run_migrations().await?;
        
        Ok(db)
    }

    async fn run_migrations(&self) -> Result<()> {
        // Create chat_configs table
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS chat_configs (
                chat_guid TEXT PRIMARY KEY,
                character_prompt TEXT,
                triggers TEXT, -- JSON array
                trigger_name TEXT DEFAULT 'myai',
                use_ollama BOOLEAN DEFAULT FALSE,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            )
        "#)
        .execute(&self.pool)
        .await
        .context("Failed to create chat_configs table")?;

        // Create chat_contexts table
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS chat_contexts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chat_guid TEXT NOT NULL,
                role TEXT NOT NULL, -- 'user', 'assistant', 'system'
                content TEXT NOT NULL,
                timestamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (chat_guid) REFERENCES chat_configs (chat_guid)
            )
        "#)
        .execute(&self.pool)
        .await
        .context("Failed to create chat_contexts table")?;

        // Create processed_messages table to track what we've already handled
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS processed_messages (
                message_guid TEXT PRIMARY KEY,
                chat_guid TEXT NOT NULL,
                processed_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            )
        "#)
        .execute(&self.pool)
        .await
        .context("Failed to create processed_messages table")?;

        // Create message_queue table for async message processing
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS message_queue (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chat_guid TEXT NOT NULL,
                message_text TEXT NOT NULL,
                queued_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                processing_started_at TIMESTAMP,
                status TEXT DEFAULT 'pending' -- 'pending', 'processing', 'completed', 'failed'
            )
        "#)
        .execute(&self.pool)
        .await
        .context("Failed to create message_queue table")?;

        // Migration: Add trigger_name column if it doesn't exist
        sqlx::query(r#"
            ALTER TABLE chat_configs ADD COLUMN trigger_name TEXT DEFAULT 'myai'
        "#)
        .execute(&self.pool)
        .await
        .ok(); // Ignore error if column already exists

        Ok(())
    }

    pub async fn get_chat_config(&self, chat_guid: &str) -> Result<Option<ChatConfig>> {
        let row = sqlx::query(
            "SELECT chat_guid, character_prompt, triggers, trigger_name, use_ollama, created_at, updated_at 
             FROM chat_configs WHERE chat_guid = ?"
        )
        .bind(chat_guid)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to fetch chat config")?;

        if let Some(row) = row {
            let triggers_json: String = row.get("triggers");
            let triggers: Vec<String> = serde_json::from_str(&triggers_json)
                .unwrap_or_else(|_| vec![]);

            Ok(Some(ChatConfig {
                chat_guid: row.get("chat_guid"),
                character_prompt: row.get("character_prompt"),
                triggers,
                trigger_name: row.get("trigger_name"),
                use_ollama: row.get("use_ollama"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn save_chat_config(&self, config: &ChatConfig) -> Result<()> {
        let triggers_json = serde_json::to_string(&config.triggers)?;
        
        sqlx::query(r#"
            INSERT OR REPLACE INTO chat_configs 
            (chat_guid, character_prompt, triggers, trigger_name, use_ollama, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
        "#)
        .bind(&config.chat_guid)
        .bind(&config.character_prompt)
        .bind(&triggers_json)
        .bind(&config.trigger_name)
        .bind(config.use_ollama)
        .bind(config.created_at)
        .bind(Utc::now())
        .execute(&self.pool)
        .await
        .context("Failed to save chat config")?;

        Ok(())
    }

    pub async fn save_message(&self, chat_guid: &str, message: &Message) -> Result<()> {
        let role_str = match message.role {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::System => "system",
        };

        sqlx::query(
            "INSERT INTO chat_contexts (chat_guid, role, content, timestamp) VALUES (?, ?, ?, ?)"
        )
        .bind(chat_guid)
        .bind(role_str)
        .bind(&message.content)
        .bind(message.timestamp)
        .execute(&self.pool)
        .await
        .context("Failed to save message")?;

        Ok(())
    }

    pub async fn get_recent_messages(&self, chat_guid: &str, limit: i64) -> Result<Vec<Message>> {
        let rows = sqlx::query(
            "SELECT role, content, timestamp FROM chat_contexts 
             WHERE chat_guid = ? 
             ORDER BY timestamp DESC 
             LIMIT ?"
        )
        .bind(chat_guid)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("Failed to fetch recent messages")?;

        let mut messages = Vec::new();
        for row in rows.into_iter().rev() { // Reverse to get chronological order
            let role_str: String = row.get("role");
            let role = match role_str.as_str() {
                "user" => MessageRole::User,
                "assistant" => MessageRole::Assistant,
                "system" => MessageRole::System,
                _ => MessageRole::User, // Default fallback
            };

            messages.push(Message {
                role,
                content: row.get("content"),
                timestamp: row.get("timestamp"),
            });
        }

        Ok(messages)
    }

    pub async fn is_message_processed(&self, message_guid: &str) -> Result<bool> {
        let row = sqlx::query("SELECT 1 FROM processed_messages WHERE message_guid = ?")
            .bind(message_guid)
            .fetch_optional(&self.pool)
            .await
            .context("Failed to check if message is processed")?;

        Ok(row.is_some())
    }

    pub async fn mark_message_processed(&self, message_guid: &str, chat_guid: &str) -> Result<()> {
        sqlx::query(
            "INSERT OR IGNORE INTO processed_messages (message_guid, chat_guid) VALUES (?, ?)"
        )
        .bind(message_guid)
        .bind(chat_guid)
        .execute(&self.pool)
        .await
        .context("Failed to mark message as processed")?;

        Ok(())
    }

    pub async fn cleanup_old_messages(&self, days: i64) -> Result<()> {
        let cutoff = Utc::now() - chrono::Duration::days(days);
        
        sqlx::query("DELETE FROM chat_contexts WHERE timestamp < ?")
            .bind(cutoff)
            .execute(&self.pool)
            .await
            .context("Failed to cleanup old messages")?;

        sqlx::query("DELETE FROM processed_messages WHERE processed_at < ?")
            .bind(cutoff)
            .execute(&self.pool)
            .await
            .context("Failed to cleanup old processed messages")?;

        Ok(())
    }

    pub async fn queue_message(&self, chat_guid: &str, message_text: &str) -> Result<i64> {
        let row = sqlx::query(
            "INSERT INTO message_queue (chat_guid, message_text) VALUES (?, ?) RETURNING id"
        )
        .bind(chat_guid)
        .bind(message_text)
        .fetch_one(&self.pool)
        .await
        .context("Failed to queue message")?;

        Ok(row.get("id"))
    }

    pub async fn get_next_queued_message(&self) -> Result<Option<(i64, String, String)>> {
        let row = sqlx::query(
            "SELECT id, chat_guid, message_text FROM message_queue 
             WHERE status = 'pending' 
             ORDER BY queued_at ASC 
             LIMIT 1"
        )
        .fetch_optional(&self.pool)
        .await
        .context("Failed to get next queued message")?;

        if let Some(row) = row {
            let id: i64 = row.get("id");
            let chat_guid: String = row.get("chat_guid");
            let message_text: String = row.get("message_text");
            
            // Mark as processing
            sqlx::query(
                "UPDATE message_queue SET status = 'processing', processing_started_at = CURRENT_TIMESTAMP WHERE id = ?"
            )
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to mark message as processing")?;

            Ok(Some((id, chat_guid, message_text)))
        } else {
            Ok(None)
        }
    }

    pub async fn mark_queue_item_completed(&self, id: i64) -> Result<()> {
        sqlx::query("UPDATE message_queue SET status = 'completed' WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to mark queue item as completed")?;
        Ok(())
    }

    pub async fn mark_queue_item_failed(&self, id: i64) -> Result<()> {
        sqlx::query("UPDATE message_queue SET status = 'failed' WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to mark queue item as failed")?;
        Ok(())
    }

    pub async fn cleanup_old_queue_items(&self, days: i64) -> Result<()> {
        let cutoff = Utc::now() - chrono::Duration::days(days);
        
        sqlx::query("DELETE FROM message_queue WHERE queued_at < ? AND status IN ('completed', 'failed')")
            .bind(cutoff)
            .execute(&self.pool)
            .await
            .context("Failed to cleanup old queue items")?;

        Ok(())
    }
}