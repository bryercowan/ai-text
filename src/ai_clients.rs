use crate::types::{Message, MessageRole};
use anyhow::{Context, Result};
use base64::Engine;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, error, info};

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum OpenAIContent {
    Text(String),
    Vision(Vec<OpenAIContentPart>),
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenAIMessage {
    pub role: String,
    pub content: Vec<OpenAIContentPart>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpenAIContentPart {
    Text { text: String },
    ImageUrl { image_url: OpenAIImageUrl },
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenAIImageUrl {
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenAIChatRequest {
    pub model: String,
    pub messages: Vec<OpenAIMessage>,
    pub temperature: f32,
    pub tools: Option<Vec<OpenAITool>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenAITool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: OpenAIFunction,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenAIFunction {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenAIChatResponse {
    pub choices: Vec<OpenAIChoice>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenAIChoice {
    pub message: OpenAIResponseMessage,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenAIResponseMessage {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<OpenAIToolCall>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenAIToolCall {
    pub function: OpenAIFunctionCall,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenAIFunctionCall {
    pub name: String,
    pub arguments: String,
}

// Ollama API structures
#[derive(Debug, Clone, Serialize)]
pub struct OllamaMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OllamaChatRequest {
    pub model: String,
    pub messages: Vec<OllamaMessage>,
    pub stream: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OllamaChatResponse {
    pub message: OllamaResponseMessage,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OllamaResponseMessage {
    pub content: String,
}

// Image generation structures
#[derive(Debug, Clone, Serialize)]
pub struct ImageGenerationRequest {
    pub model: String,
    pub prompt: String,
    pub n: u32,
    pub size: String,
    pub quality: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImageGenerationResponse {
    pub data: Vec<ImageData>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImageData {
    pub url: String,
}

#[derive(Clone)]
pub struct AIClients {
    http_client: Client,
    openai_api_key: Option<String>,
    ollama_api: String,
    ollama_model: String,
}

impl AIClients {
    pub fn new(openai_api_key: Option<String>, ollama_api: String, ollama_model: String) -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            http_client,
            openai_api_key,
            ollama_api,
            ollama_model,
        }
    }

    pub async fn generate_chat_completion(
        &self,
        messages: &[Message],
        system_prompt: &str,
        use_ollama: bool,
        include_image_tool: bool,
        image_data: Option<Vec<u8>>,
    ) -> Result<String> {
        if use_ollama {
            self.ollama_chat_completion(messages, system_prompt, image_data)
                .await
        } else {
            self.openai_chat_completion(messages, system_prompt, include_image_tool, image_data)
                .await
        }
    }

    pub async fn generate_character_prompt(&self, description: &str) -> Result<String> {
        let system_prompt = "You are a prompt engineer. Generate a detailed system prompt for an AI character based on the user's description. The prompt should:
1. Define the character's personality, mannerisms, and speaking style
2. Include specific behavioral traits and quirks
3. Be detailed enough to create a consistent character persona
4. Start with \"You are [character description]...\"

Keep it concise but comprehensive. Return only the system prompt, nothing else.";

        let messages = vec![Message {
            role: MessageRole::User,
            content: description.to_string(),
            timestamp: chrono::Utc::now(),
        }];

        let prompt = if self.openai_api_key.is_some() {
            self.openai_chat_completion(&messages, system_prompt, false, None)
                .await?
        } else {
            self.ollama_chat_completion(&messages, system_prompt, None)
                .await?
        };

        Ok(prompt.trim().to_string())
    }

    async fn openai_chat_completion(
        &self,
        messages: &[Message],
        system_prompt: &str,
        include_image_tool: bool,
        image_data: Option<Vec<u8>>,
    ) -> Result<String> {
        let api_key = self
            .openai_api_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("OpenAI API key not configured"))?;

        let mut openai_messages = vec![OpenAIMessage {
            role: "system".to_string(),
            content: vec![OpenAIContentPart::Text {
                text: if include_image_tool {
                    format!("{} If you want to generate and send a picture or image, use the request_picture tool with a detailed description of what image you want to create.", system_prompt)
                } else {
                    system_prompt.to_string()
                },
            }],
        }];

        for message in messages {
            let role = match message.role {
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::System => "system",
            };
            openai_messages.push(OpenAIMessage {
                role: role.to_string(),
                content: vec![OpenAIContentPart::Text {
                    text: message.content.clone(),
                }],
            });
        }

        let has_image = image_data.is_some();

        // Add image if provided - create a separate user message with vision content
        if let Some(image_bytes) = image_data {
            let base64_image = base64::engine::general_purpose::STANDARD.encode(&image_bytes);
            let data_url = format!("data:image/jpeg;base64,{}", base64_image);

            // Add a new user message specifically for image analysis
            openai_messages.push(OpenAIMessage {
                role: "user".to_string(),
                content: vec![
                    OpenAIContentPart::Text {
                        text: "what's in this image?".to_string(),
                    },
                    OpenAIContentPart::ImageUrl {
                        image_url: OpenAIImageUrl { url: data_url },
                    },
                ],
            });
        }

        let model = if has_image {
            "gpt-4o".to_string() // gpt-4o supports vision
        } else {
            "gpt-4o".to_string()
        };

        let mut request = OpenAIChatRequest {
            model,
            messages: openai_messages,
            temperature: 0.7,
            tools: None,
        };

        if include_image_tool {
            request.tools = Some(vec![OpenAITool {
                tool_type: "function".to_string(),
                function: OpenAIFunction {
                    name: "request_picture".to_string(),
                    description: "Generate and send a picture to the chat using DALL-E".to_string(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "description": {
                                "type": "string",
                                "description": "Detailed description of the picture to generate using DALL-E"
                            }
                        },
                        "required": ["description"]
                    }),
                },
            }]);
        }

        debug!("Sending OpenAI chat completion request");

        let response = self
            .http_client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .context("Failed to send OpenAI request")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            error!("OpenAI API failed with status {}: {}", status, text);
            return Err(anyhow::anyhow!("OpenAI API failed: {}", text));
        }

        let chat_response: OpenAIChatResponse = response
            .json()
            .await
            .context("Failed to parse OpenAI response")?;

        if let Some(choice) = chat_response.choices.first() {
            if let Some(tool_calls) = &choice.message.tool_calls {
                // Handle tool calls (image generation)
                for tool_call in tool_calls {
                    if tool_call.function.name == "request_picture" {
                        return Ok(format!(
                            "[TOOL_CALL:request_picture:{}]",
                            tool_call.function.arguments
                        ));
                    }
                }
            }

            Ok(choice.message.content.clone().unwrap_or_default())
        } else {
            Err(anyhow::anyhow!("No choices in OpenAI response"))
        }
    }

    async fn ollama_chat_completion(
        &self,
        messages: &[Message],
        system_prompt: &str,
        image_data: Option<Vec<u8>>,
    ) -> Result<String> {
        let mut ollama_messages = vec![OllamaMessage {
            role: "system".to_string(),
            content: if image_data.is_some() {
                format!("{} If you want to generate and send a picture, just say [REQUEST_PICTURE] followed by your description. Note: An image was uploaded but Ollama vision support is limited.", system_prompt)
            } else {
                format!("{} If you want to generate and send a picture, just say [REQUEST_PICTURE] followed by your description.", system_prompt)
            },
        }];

        for message in messages {
            let role = match message.role {
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::System => "system",
            };
            ollama_messages.push(OllamaMessage {
                role: role.to_string(),
                content: message.content.clone(),
            });
        }

        let request = OllamaChatRequest {
            model: self.ollama_model.clone(),
            messages: ollama_messages,
            stream: false,
        };

        debug!(
            "Sending Ollama chat completion request to {}",
            self.ollama_api
        );

        let response = self
            .http_client
            .post(&format!("{}/api/chat", self.ollama_api))
            .json(&request)
            .send()
            .await
            .context("Failed to send Ollama request")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            error!("Ollama API failed with status {}: {}", status, text);
            return Err(anyhow::anyhow!("Ollama API failed: {}", text));
        }

        let chat_response: OllamaChatResponse = response
            .json()
            .await
            .context("Failed to parse Ollama response")?;

        let content = chat_response.message.content;

        // Check for picture generation request
        if content.contains("[REQUEST_PICTURE]") {
            let description = content.replace("[REQUEST_PICTURE]", "").trim().to_string();
            return Ok(format!(
                "[TOOL_CALL:request_picture:{}]",
                serde_json::json!({"description": description}).to_string()
            ));
        }

        Ok(content)
    }

    pub async fn generate_image(&self, description: &str) -> Result<Vec<u8>> {
        let api_key = self
            .openai_api_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("OpenAI API key required for image generation"))?;

        let request = ImageGenerationRequest {
            model: "dall-e-3".to_string(),
            prompt: description.to_string(),
            n: 1,
            size: "1024x1024".to_string(),
            quality: "standard".to_string(),
        };

        debug!("Generating image with DALL-E: {}", description);

        let response = self
            .http_client
            .post("https://api.openai.com/v1/images/generations")
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .context("Failed to send image generation request")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            error!("Image generation failed with status {}: {}", status, text);
            return Err(anyhow::anyhow!("Image generation failed: {}", text));
        }

        let image_response: ImageGenerationResponse = response
            .json()
            .await
            .context("Failed to parse image generation response")?;

        let image_url = &image_response
            .data
            .first()
            .ok_or_else(|| anyhow::anyhow!("No image data in response"))?
            .url;

        // Download the generated image
        let image_response = self
            .http_client
            .get(image_url)
            .send()
            .await
            .context("Failed to download generated image")?;

        let image_bytes = image_response
            .bytes()
            .await
            .context("Failed to read image bytes")?;

        info!(
            "Successfully generated and downloaded image ({} bytes)",
            image_bytes.len()
        );
        Ok(image_bytes.to_vec())
    }
}
