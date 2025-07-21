use anyhow::{Result, Context};
use reqwest::{Client, multipart};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, error, info};
use crate::types::{BlueBubblesChat, BlueBubblesMessage};

#[derive(Debug, Clone, Serialize)]
struct ChatQuery {
    limit: u32,
    offset: u32,
    with: Vec<String>,
    sort: String,
}

#[derive(Debug, Clone, Serialize)]
struct MessageQuery {
    #[serde(rename = "chatGuid")]
    chat_guid: String,
    limit: u32,
    offset: u32,
    sort: String,
    #[serde(rename = "after")]
    after: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
struct SendMessageRequest {
    #[serde(rename = "chatGuid")]
    chat_guid: String,
    message: String,
    #[serde(rename = "tempGuid")]
    temp_guid: String,
}

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    data: Option<T>,
    message: Option<String>,
    error: Option<String>,
}

pub struct BlueBubblesClient {
    client: Client,
    base_url: String,
    password: Option<String>,
}

impl BlueBubblesClient {
    pub fn new(base_url: String, password: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url,
            password,
        }
    }

    fn build_url(&self, endpoint: &str) -> String {
        let password_param = match &self.password {
            Some(password) => format!("?password={}", password),
            None => "".to_string(),
        };
        
        format!("{}/api/v1{}{}", self.base_url, endpoint, password_param)
    }

    pub async fn get_chats(&self) -> Result<Vec<BlueBubblesChat>> {
        let url = self.build_url("/chat/query");
        
        let query = ChatQuery {
            limit: 500,
            offset: 0,
            with: vec!["lastMessage".to_string()],
            sort: "lastmessage".to_string(),
        };

        debug!("Fetching chats from: {}", url);

        let response = self.client
            .post(&url)
            .json(&query)
            .send()
            .await
            .context("Failed to send chat query request")?;

        if !response.status().is_success() {
            error!("Chat query failed with status: {}", response.status());
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Chat query failed: {}", text));
        }

        let api_response: ApiResponse<Vec<BlueBubblesChat>> = response
            .json()
            .await
            .context("Failed to parse chat query response")?;

        match api_response.data {
            Some(chats) => {
                debug!("Retrieved {} chats", chats.len());
                Ok(chats)
            }
            None => {
                error!("No chat data in response: {:?}", api_response.error);
                Ok(vec![])
            }
        }
    }

    pub async fn get_messages(&self, chat_guid: &str) -> Result<Vec<BlueBubblesMessage>> {
        self.get_messages_after(chat_guid, None).await
    }

    pub async fn get_messages_after(&self, chat_guid: &str, after_timestamp: Option<u64>) -> Result<Vec<BlueBubblesMessage>> {
        let url = self.build_url("/message/query");
        
        let query = MessageQuery {
            chat_guid: chat_guid.to_string(),
            limit: 50,
            offset: 0,
            sort: "DESC".to_string(),
            after: after_timestamp,
        };

        match after_timestamp {
            Some(ts) => debug!("Fetching messages for chat {} after {} from: {}", chat_guid, ts, url),
            None => debug!("Fetching messages for chat {} from: {}", chat_guid, url),
        }

        let response = self.client
            .post(&url)
            .json(&query)
            .send()
            .await
            .context("Failed to send message query request")?;

        if !response.status().is_success() {
            error!("Message query failed with status: {}", response.status());
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Message query failed: {}", text));
        }

        let api_response: ApiResponse<Vec<BlueBubblesMessage>> = response
            .json()
            .await
            .context("Failed to parse message query response")?;

        match api_response.data {
            Some(messages) => {
                debug!("Retrieved {} messages for chat {}", messages.len(), chat_guid);
                Ok(messages)
            }
            None => {
                error!("No message data in response: {:?}", api_response.error);
                Ok(vec![])
            }
        }
    }

    pub async fn send_message(&self, chat_guid: &str, message: &str) -> Result<()> {
        let url = self.build_url("/message/text");
        
        let temp_guid = format!("temp-{}-{}", 
            chrono::Utc::now().timestamp_millis(),
            uuid::Uuid::new_v4().to_string().chars().take(9).collect::<String>()
        );

        let request = SendMessageRequest {
            chat_guid: chat_guid.to_string(),
            message: message.to_string(),
            temp_guid,
        };

        debug!("Sending message to chat {}: {}", chat_guid, message);

        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("Failed to send message request")?;

        if !response.status().is_success() {
            error!("Send message failed with status: {}", response.status());
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Send message failed: {}", text));
        }

        info!("Message sent successfully to chat {}", chat_guid);
        Ok(())
    }

    pub async fn send_attachment(&self, chat_guid: &str, image_data: Vec<u8>, filename: &str) -> Result<()> {
        let url = self.build_url("/message/attachment");
        
        let temp_guid = format!("temp-{}-{}", 
            chrono::Utc::now().timestamp_millis(),
            uuid::Uuid::new_v4().to_string().chars().take(9).collect::<String>()
        );

        let form = multipart::Form::new()
            .text("chatGuid", chat_guid.to_string())
            .text("tempGuid", temp_guid)
            .text("name", filename.to_string())
            .part("attachment", multipart::Part::bytes(image_data)
                .file_name(filename.to_string())
                .mime_str("image/png")?);

        debug!("Sending attachment to chat {}: {}", chat_guid, filename);

        let response = self.client
            .post(&url)
            .multipart(form)
            .send()
            .await
            .context("Failed to send attachment request")?;

        if !response.status().is_success() {
            error!("Send attachment failed with status: {}", response.status());
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Send attachment failed: {}", text));
        }

        info!("Attachment sent successfully to chat {}", chat_guid);
        Ok(())
    }
}