use anyhow::Result;
use chrono::Utc;
use std::collections::VecDeque;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::{
    ai_clients::AIClients,
    bluebubbles::BlueBubblesClient,
    commands::CommandHandler,
    config::Config,
    database::Database,
    types::{ChatConfig, Message, MessageRole, QueuedMessage},
};

#[derive(Debug, Clone)]
pub enum ChatAgentMessage {
    ProcessMessage(QueuedMessage),
    Shutdown,
}

pub struct ChatAgent {
    chat_guid: String,
    config: ChatConfig,
    context: VecDeque<Message>,
    ai_clients: AIClients,
    bluebubbles: BlueBubblesClient,
    database: Database,
    command_handler: CommandHandler,
    receiver: mpsc::Receiver<ChatAgentMessage>,
}

impl ChatAgent {
    pub async fn new(
        chat_guid: String,
        global_config: &Config,
        database: Database,
        receiver: mpsc::Receiver<ChatAgentMessage>,
    ) -> Result<Self> {
        // Load chat-specific config from database or create default
        let config = database
            .get_chat_config(&chat_guid)
            .await?
            .unwrap_or_else(|| ChatConfig {
                chat_guid: chat_guid.clone(),
                character_prompt: None,
                triggers: global_config.triggers(),
                trigger_name: "myai".to_string(),
                use_ollama: false,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            });

        // Load recent messages from database to populate context
        let recent_messages = database.get_recent_messages(&chat_guid, 10).await?;
        let mut context = VecDeque::new();
        for message in recent_messages {
            context.push_back(message);
        }

        let ai_clients = AIClients::new(
            global_config.openai_api_key.clone(),
            global_config.ollama_api.clone(),
            global_config.ollama_model.clone(),
        );

        let bluebubbles = BlueBubblesClient::new(
            global_config.bluebubbles_api.clone(),
            global_config.bluebubbles_password.clone(),
        );

        let command_handler = CommandHandler::new(ai_clients.clone(), database.clone())?;

        Ok(Self {
            chat_guid,
            config,
            context,
            ai_clients,
            bluebubbles,
            database,
            command_handler,
            receiver,
        })
    }

    pub async fn run(mut self) -> Result<()> {
        info!("Starting chat agent for chat: {}", self.chat_guid);

        while let Some(message) = self.receiver.recv().await {
            match message {
                ChatAgentMessage::ProcessMessage(queued_message) => {
                    if let Err(e) = self.handle_message(queued_message).await {
                        error!("Error handling message in chat {}: {}", self.chat_guid, e);
                        
                        // Send error message to chat
                        if let Err(send_error) = self.bluebubbles.send_message(
                            &self.chat_guid,
                            "❌ Error processing message. Please try again."
                        ).await {
                            error!("Failed to send error message: {}", send_error);
                        }
                    }
                }
                ChatAgentMessage::Shutdown => {
                    info!("Shutting down chat agent for chat: {}", self.chat_guid);
                    break;
                }
            }
        }

        Ok(())
    }

    async fn handle_message(&mut self, queued_message: QueuedMessage) -> Result<()> {
        let text = &queued_message.text;
        debug!("Processing message in chat {}: {}", self.chat_guid, text);

        // Try to handle as a command first
        if let Some(response) = self.command_handler
            .handle_command(&self.chat_guid, text, &mut self.config)
            .await? 
        {
            // It was a command, send the response and clear context if needed
            self.bluebubbles.send_message(&self.chat_guid, &response).await?;
            
            // If it was a character command, clear the context
            if text.to_lowercase().starts_with("@character") {
                self.context.clear();
            }
            
            return Ok(());
        }

        // Not a command, process as regular message
        let user_message = Message {
            role: MessageRole::User,
            content: text.clone(),
            timestamp: queued_message.timestamp,
        };

        // Add to context
        self.context.push_back(user_message.clone());
        
        // Keep only last 10 messages
        while self.context.len() > 10 {
            self.context.pop_front();
        }

        // Ensure chat config is saved first (for foreign key constraint)
        self.database.save_chat_config(&self.config).await?;
        
        // Save user message to database
        self.database.save_message(&self.chat_guid, &user_message).await?;

        // Generate AI response
        let system_prompt = self.config.character_prompt
            .as_ref()
            .map(|s| s.as_str())
            .unwrap_or("You are MyAI, a casual assistant in a private friend group chat. Be brief and natural unless asked to elaborate. Match the group's tone and energy.");

        let context_messages: Vec<_> = self.context.iter().cloned().collect();
        
        let ai_response = self.ai_clients
            .generate_chat_completion(&context_messages, system_prompt, self.config.use_ollama, true)
            .await?;

        // Check if AI wants to generate an image
        if ai_response.starts_with("[TOOL_CALL:request_picture:") {
            let end_idx = ai_response.find(']').unwrap_or(ai_response.len());
            let args_json = &ai_response[27..end_idx]; // Skip "[TOOL_CALL:request_picture:"
            
            if let Ok(args) = serde_json::from_str::<serde_json::Value>(args_json) {
                if let Some(description) = args.get("description").and_then(|v| v.as_str()) {
                    match self.generate_and_send_image(description).await {
                        Ok(_) => {
                            let response_text = "✅ Generated and sent a picture!";
                            self.bluebubbles.send_message(&self.chat_guid, response_text).await?;
                            
                            let assistant_message = Message {
                                role: MessageRole::Assistant,
                                content: response_text.to_string(),
                                timestamp: Utc::now(),
                            };
                            self.context.push_back(assistant_message.clone());
                            self.database.save_message(&self.chat_guid, &assistant_message).await?;
                            return Ok(());
                        }
                        Err(e) => {
                            error!("Failed to generate image: {}", e);
                            let error_text = "❌ Failed to generate image. Please try again.";
                            self.bluebubbles.send_message(&self.chat_guid, error_text).await?;
                            
                            let assistant_message = Message {
                                role: MessageRole::Assistant,
                                content: error_text.to_string(),
                                timestamp: Utc::now(),
                            };
                            self.context.push_back(assistant_message.clone());
                            self.database.save_message(&self.chat_guid, &assistant_message).await?;
                            return Ok(());
                        }
                    }
                }
            }
        }

        // Regular text response
        let response_text = ai_response;

        self.bluebubbles.send_message(&self.chat_guid, &response_text).await?;

        // Add assistant response to context
        let assistant_message = Message {
            role: MessageRole::Assistant,
            content: response_text.clone(),
            timestamp: Utc::now(),
        };

        self.context.push_back(assistant_message.clone());
        self.database.save_message(&self.chat_guid, &assistant_message).await?;

        debug!("Successfully processed message in chat {}", self.chat_guid);
        Ok(())
    }

    async fn generate_and_send_image(&self, description: &str) -> Result<()> {
        info!("Generating image for chat {}: {}", self.chat_guid, description);

        // Generate the image
        let image_data = self.ai_clients.generate_image(description).await?;

        // Send the image to the chat
        self.bluebubbles
            .send_attachment(&self.chat_guid, image_data, "generated-image.png")
            .await?;

        info!("Successfully generated and sent image to chat {}", self.chat_guid);
        Ok(())
    }
}

pub struct ChatAgentHandle {
    pub chat_guid: String,
    pub sender: mpsc::Sender<ChatAgentMessage>,
    pub task_handle: tokio::task::JoinHandle<Result<()>>,
}

impl ChatAgentHandle {
    pub async fn send_message(&self, message: QueuedMessage) -> Result<()> {
        self.sender
            .send(ChatAgentMessage::ProcessMessage(message))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send message to chat agent: {}", e))?;
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.sender
            .send(ChatAgentMessage::Shutdown)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send shutdown message to chat agent: {}", e))?;
        Ok(())
    }
}