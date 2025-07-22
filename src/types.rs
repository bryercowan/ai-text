use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlueBubblesMessage {
    pub guid: String,
    pub text: Option<String>,
    #[serde(rename = "dateCreated")]
    pub date_created: Option<i64>,
    #[serde(rename = "dateDelivered")]
    pub date_delivered: Option<i64>,
    #[serde(rename = "isFromMe")]
    pub is_from_me: Option<bool>,
    pub attachments: Option<Vec<BlueBubblesAttachment>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlueBubblesAttachment {
    pub guid: String,
    #[serde(rename = "originalROWID")]
    pub original_rowid: Option<i64>,
    #[serde(rename = "mimeType")]
    pub mime_type: Option<String>,
    #[serde(rename = "transferName")]
    pub transfer_name: Option<String>,
    #[serde(rename = "totalBytes")]
    pub total_bytes: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlueBubblesChat {
    pub guid: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(rename = "lastMessage")]
    pub last_message: Option<BlueBubblesMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatConfig {
    pub chat_guid: String,
    pub character_prompt: Option<String>,
    pub triggers: Vec<String>,
    pub trigger_name: String, // NLP trigger name like "myai", "bot", "assistant"
    pub use_ollama: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ChatContext {
    pub chat_guid: String,
    pub messages: VecDeque<Message>,
    pub character_prompt: Option<String>,
    pub use_ollama: bool,
    pub triggers: Vec<String>,
}

impl ChatContext {
    pub fn new(chat_guid: String, config: Option<ChatConfig>) -> Self {
        let (character_prompt, use_ollama, triggers) = match config {
            Some(config) => (config.character_prompt, config.use_ollama, config.triggers),
            None => (None, false, vec![]),
        };

        Self {
            chat_guid,
            messages: VecDeque::new(),
            character_prompt,
            use_ollama,
            triggers,
        }
    }

    pub fn add_message(&mut self, message: Message) {
        self.messages.push_back(message);
        
        // Keep only last 10 messages
        while self.messages.len() > 10 {
            self.messages.pop_front();
        }
    }

    pub fn get_system_prompt(&self) -> String {
        self.character_prompt.clone().unwrap_or_else(|| {
            "You are MyAI, a casual assistant in a private friend group chat. Be brief and natural unless asked to elaborate. Match the group's tone and energy.".to_string()
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedMessage {
    pub id: Uuid,
    pub chat_guid: String,
    pub text: String,
    pub timestamp: DateTime<Utc>,
}

impl QueuedMessage {
    pub fn new(chat_guid: String, text: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            chat_guid,
            text,
            timestamp: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaMessage {
    pub role: String,
    pub content: String,
}