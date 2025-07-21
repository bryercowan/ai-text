use anyhow::{Context, Result};
use dashmap::DashMap;
use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::{sync::mpsc, time::interval};
use tracing::{debug, error, info, warn};

use crate::{
    bluebubbles::BlueBubblesClient,
    chat_agent::{ChatAgent, ChatAgentHandle},
    config::Config,
    database::Database,
    types::QueuedMessage,
};

pub struct BotOrchestrator {
    config: Config,
    database: Database,
    bluebubbles: BlueBubblesClient,
    chat_agents: DashMap<String, ChatAgentHandle>,
    processed_messages: HashSet<String>,
    startup_time: u64,
}

impl BotOrchestrator {
    pub async fn new(config: Config) -> Result<Self> {
        let database = Database::new(&config.database_url)
            .await
            .context("Failed to initialize database")?;

        let bluebubbles = BlueBubblesClient::new(
            config.bluebubbles_api.clone(),
            config.bluebubbles_password.clone(),
        );

        let startup_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64;

        Ok(Self {
            config,
            database,
            bluebubbles,
            chat_agents: DashMap::new(),
            processed_messages: HashSet::new(),
            startup_time,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        info!("Starting bot orchestrator");

        let mut poll_interval = interval(Duration::from_secs(3));
        let mut queue_interval = interval(Duration::from_millis(500)); // Process queue more frequently
        let mut cleanup_interval = interval(Duration::from_secs(300)); // 5 minutes

        loop {
            tokio::select! {
                _ = poll_interval.tick() => {
                    if let Err(e) = self.poll_and_process_messages().await {
                        error!("Error during message polling: {}", e);
                    }
                }
                _ = queue_interval.tick() => {
                    if let Err(e) = self.process_message_queue().await {
                        error!("Error during queue processing: {}", e);
                    }
                }
                _ = cleanup_interval.tick() => {
                    if let Err(e) = self.cleanup().await {
                        error!("Error during cleanup: {}", e);
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("Received shutdown signal, stopping orchestrator");
                    self.shutdown().await?;
                    break;
                }
            }
        }

        Ok(())
    }

    async fn poll_and_process_messages(&mut self) -> Result<()> {
        info!("Polling for new messages");

        let chats = self
            .bluebubbles
            .get_chats()
            .await
            .context("Failed to get chats from BlueBubbles")?;

        for chat in chats {
            let messages = self
                .bluebubbles
                .get_messages_after(&chat.guid, Some(self.startup_time))
                .await
                .context("Failed to get messages from BlueBubbles")?;

            for message in messages.into_iter().rev() {
                // Process in chronological order
                // Skip if we've already processed this message
                if self.processed_messages.contains(&message.guid) {
                    continue;
                }

                // Skip messages from us
                if message.is_from_me == Some(true) {
                    self.processed_messages.insert(message.guid);
                    continue;
                }
                //
                // Skip messages older than startup time
                let message_time =
                    message.date_created.or(message.date_delivered).unwrap_or(0) as u64;

                if message_time < self.startup_time {
                    self.processed_messages.insert(message.guid);
                    continue;
                }

                // Check if message already processed in database
                if self.database.is_message_processed(&message.guid).await? {
                    self.processed_messages.insert(message.guid);
                    continue;
                }

                let text = message.text.unwrap_or_default();
                if text.is_empty() {
                    self.processed_messages.insert(message.guid);
                    continue;
                }

                debug!("Processing message from chat {}: '{}'", chat.guid, text);

                // Check for triggers - both @ commands and NLP triggers
                let contains_trigger = self.check_message_triggers(&chat.guid, &text).await?;

                debug!("Message contains trigger: {}", contains_trigger);

                if contains_trigger {
                    info!("Found triggered message in chat {}: {}", chat.guid, text);

                    // Mark as processed
                    self.processed_messages.insert(message.guid.clone());
                    self.database
                        .mark_message_processed(&message.guid, &chat.guid)
                        .await?;

                    // Queue the message for processing
                    if let Err(e) = self.database.queue_message(&chat.guid, &text).await {
                        error!("Failed to queue message for chat {}: {}", chat.guid, e);
                    }
                } else {
                    self.processed_messages.insert(message.guid);
                }
            }
        }

        // Cleanup processed messages set if it gets too large
        if self.processed_messages.len() > 1000 {
            let messages_vec: Vec<_> = self.processed_messages.iter().cloned().collect();
            let keep = messages_vec.into_iter().skip(500).collect();
            self.processed_messages = keep;
        }

        debug!("Finished polling messages");
        Ok(())
    }

    async fn process_message_queue(&mut self) -> Result<()> {
        // Process up to 3 messages from the queue in this tick
        for _ in 0..3 {
            if let Some((queue_id, chat_guid, message_text)) =
                self.database.get_next_queued_message().await?
            {
                debug!(
                    "Processing queued message {} for chat {}: {}",
                    queue_id, chat_guid, message_text
                );

                // Ensure chat agent exists
                if let Err(e) = self.ensure_chat_agent(&chat_guid).await {
                    error!("Failed to create chat agent for {}: {}", chat_guid, e);
                    self.database.mark_queue_item_failed(queue_id).await?;
                    continue;
                }

                // Send message to chat agent
                let queued_message = QueuedMessage::new(chat_guid.clone(), message_text);

                if let Some(agent_handle) = self.chat_agents.get(&chat_guid) {
                    match agent_handle.send_message(queued_message).await {
                        Ok(_) => {
                            debug!("Successfully sent queued message {} to agent", queue_id);
                            self.database.mark_queue_item_completed(queue_id).await?;
                        }
                        Err(e) => {
                            error!("Failed to send queued message {} to agent: {}", queue_id, e);
                            self.database.mark_queue_item_failed(queue_id).await?;
                            // Remove the failed agent so it can be recreated
                            self.remove_chat_agent(&chat_guid).await;
                        }
                    }
                } else {
                    error!(
                        "Chat agent not found for {}, marking queue item as failed",
                        chat_guid
                    );
                    self.database.mark_queue_item_failed(queue_id).await?;
                }
            } else {
                // No more messages in queue
                break;
            }
        }

        Ok(())
    }

    async fn check_message_triggers(&self, chat_guid: &str, text: &str) -> Result<bool> {
        let lower_text = text.to_lowercase();

        // Check for @ commands first (these always trigger)
        let global_triggers = self.config.triggers();
        debug!(
            "Checking message '{}' against global triggers: {:?}",
            lower_text, global_triggers
        );

        for trigger in &global_triggers {
            if lower_text.contains(&trigger.to_lowercase()) {
                debug!("Found global trigger: {}", trigger);
                return Ok(true);
            }
        }

        // Check for NLP trigger (chat-specific name)
        let trigger_name = match self.database.get_chat_config(chat_guid).await {
            Ok(Some(chat_config)) => {
                debug!(
                    "Found chat config for {}: trigger_name = {}",
                    chat_guid, chat_config.trigger_name
                );
                chat_config.trigger_name.to_lowercase()
            }
            Ok(None) => {
                debug!(
                    "No chat config found for {}, using default trigger 'myai'",
                    chat_guid
                );
                "myai".to_string()
            }
            Err(e) => {
                debug!(
                    "Failed to get chat config for {}: {}, using default trigger 'myai'",
                    chat_guid, e
                );
                "myai".to_string()
            }
        };

        debug!(
            "Checking for NLP trigger '{}' in message '{}'",
            trigger_name, lower_text
        );

        // NLP matching: check if the trigger name appears as a word (not substring)
        if self.contains_trigger_word(&lower_text, &trigger_name) {
            debug!("Found NLP trigger: {}", trigger_name);
            return Ok(true);
        }

        Ok(false)
    }

    fn contains_trigger_word(&self, text: &str, trigger: &str) -> bool {
        // Convert both to lowercase for case-insensitive matching
        let text_lower = text.to_lowercase();
        let trigger_lower = trigger.to_lowercase();

        // Simple word boundary matching
        // Split text into words and check for exact match
        let words: Vec<&str> = text_lower
            .split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
            .collect();

        debug!(
            "Word analysis: text='{}' -> words={:?}, looking for trigger='{}'",
            text_lower, words, trigger_lower
        );

        let found = words.iter().any(|&word| word == trigger_lower);
        debug!("Trigger word found: {}", found);

        // Also try simple contains as fallback
        if !found {
            let contains_fallback = text_lower.contains(&trigger_lower);
            debug!("Fallback contains check: {}", contains_fallback);
            return contains_fallback;
        }

        found
    }

    async fn ensure_chat_agent(&self, chat_guid: &str) -> Result<()> {
        if !self.chat_agents.contains_key(chat_guid) {
            debug!("Creating new chat agent for chat: {}", chat_guid);

            let (sender, receiver) = mpsc::channel(100);

            let agent = ChatAgent::new(
                chat_guid.to_string(),
                &self.config,
                self.database.clone(),
                receiver,
            )
            .await?;

            let task_handle = tokio::spawn(async move { agent.run().await });

            let agent_handle = ChatAgentHandle {
                chat_guid: chat_guid.to_string(),
                sender,
                task_handle,
            };

            self.chat_agents.insert(chat_guid.to_string(), agent_handle);
            info!("Created new chat agent for chat: {}", chat_guid);
        }

        Ok(())
    }

    async fn remove_chat_agent(&self, chat_guid: &str) {
        if let Some((_, agent_handle)) = self.chat_agents.remove(chat_guid) {
            info!("Removing chat agent for chat: {}", chat_guid);

            // Try to shutdown gracefully
            if let Err(e) = agent_handle.shutdown().await {
                warn!("Failed to shutdown chat agent gracefully: {}", e);
            }

            // Abort the task if it doesn't finish quickly
            let task_handle = agent_handle.task_handle;
            tokio::select! {
                result = task_handle => {
                    match result {
                        Ok(Ok(())) => debug!("Chat agent {} shutdown successfully", chat_guid),
                        Ok(Err(e)) => error!("Chat agent {} finished with error: {}", chat_guid, e),
                        Err(e) => error!("Chat agent {} task panicked: {}", chat_guid, e),
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(5)) => {
                    warn!("Chat agent {} didn't shutdown in time, aborting", chat_guid);
                    // Task handle was moved into the select, so we can't abort it here
                    // The task will be dropped when the select completes
                }
            }
        }
    }

    async fn cleanup(&mut self) -> Result<()> {
        debug!("Running cleanup tasks");

        // Cleanup old database entries (older than 7 days)
        if let Err(e) = self.database.cleanup_old_messages(7).await {
            error!("Failed to cleanup old database messages: {}", e);
        }

        // Cleanup old queue items (older than 1 day)
        if let Err(e) = self.database.cleanup_old_queue_items(1).await {
            error!("Failed to cleanup old queue items: {}", e);
        }

        // Remove inactive chat agents (those that haven't been used recently)
        let mut to_remove = Vec::new();

        for entry in self.chat_agents.iter() {
            let chat_guid = entry.key();
            let agent_handle = entry.value();

            // Check if the task is still alive
            if agent_handle.task_handle.is_finished() {
                to_remove.push(chat_guid.clone());
            }
        }

        for chat_guid in to_remove {
            info!("Removing inactive chat agent: {}", chat_guid);
            self.remove_chat_agent(&chat_guid).await;
        }

        debug!("Cleanup completed");
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        info!("Shutting down bot orchestrator");

        // Shutdown all chat agents
        let chat_guids: Vec<String> = self
            .chat_agents
            .iter()
            .map(|entry| entry.key().clone())
            .collect();

        for chat_guid in chat_guids {
            self.remove_chat_agent(&chat_guid).await;
        }

        info!("Bot orchestrator shutdown complete");
        Ok(())
    }
}
