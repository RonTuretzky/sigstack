//! In-memory conversation storage with TTL expiration.

use crate::error::ConversationError;
use crate::types::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, info, instrument};

/// Entry in the conversation store with expiration tracking.
struct ConversationEntry {
    conversation: Conversation,
    expires_at: std::time::Instant,
}

/// In-memory conversation store with automatic TTL expiration.
///
/// All data is kept in TEE-protected memory. Conversations are
/// automatically cleaned up after the configured TTL expires.
#[derive(Clone)]
pub struct ConversationStore {
    conversations: Arc<RwLock<HashMap<String, ConversationEntry>>>,
    max_messages: usize,
    ttl: Duration,
}

impl ConversationStore {
    /// Create a new in-memory conversation store.
    ///
    /// Spawns a background task to periodically clean up expired conversations.
    pub fn new(max_messages: usize, ttl: Duration) -> Self {
        let store = Self {
            conversations: Arc::new(RwLock::new(HashMap::new())),
            max_messages,
            ttl,
        };

        // Spawn cleanup task
        let cleanup_store = store.clone();
        tokio::spawn(async move {
            cleanup_store.cleanup_loop().await;
        });

        info!(
            "In-memory conversation store initialized (max_messages={}, ttl={:?})",
            max_messages, ttl
        );

        store
    }

    /// Background task that periodically removes expired conversations.
    async fn cleanup_loop(&self) {
        let cleanup_interval = Duration::from_secs(60); // Check every minute

        loop {
            tokio::time::sleep(cleanup_interval).await;

            let now = std::time::Instant::now();
            let mut conversations = self.conversations.write().await;
            let before_count = conversations.len();

            conversations.retain(|_, entry| entry.expires_at > now);

            let removed = before_count - conversations.len();
            if removed > 0 {
                debug!("Cleaned up {} expired conversations", removed);
            }
        }
    }

    /// Get conversation for a user.
    #[instrument(skip(self))]
    pub async fn get(&self, user_id: &str) -> Result<Option<Conversation>, ConversationError> {
        let conversations = self.conversations.read().await;
        let now = std::time::Instant::now();

        Ok(conversations
            .get(user_id)
            .filter(|entry| entry.expires_at > now)
            .map(|entry| entry.conversation.clone()))
    }

    /// Add a message to a conversation, creating if needed.
    #[instrument(skip(self, content))]
    pub async fn add_message(
        &self,
        user_id: &str,
        role: &str,
        content: &str,
        system_prompt: Option<&str>,
    ) -> Result<Conversation, ConversationError> {
        let mut conversations = self.conversations.write().await;
        let now = std::time::Instant::now();
        let expires_at = now + self.ttl;

        let entry = conversations
            .entry(user_id.to_string())
            .or_insert_with(|| ConversationEntry {
                conversation: Conversation::new(user_id, system_prompt.map(String::from)),
                expires_at,
            });

        // Update expiration on activity
        entry.expires_at = expires_at;

        // Update system prompt if provided
        if let Some(prompt) = system_prompt {
            entry.conversation.system_prompt = Some(prompt.to_string());
        }

        // Add the message
        entry.conversation.add_message(role, content);

        // Trim old messages
        entry.conversation.trim(self.max_messages);

        debug!(
            "Added message for {} (total: {})",
            user_id,
            entry.conversation.messages.len()
        );

        Ok(entry.conversation.clone())
    }

    /// Clear a user's conversation.
    #[instrument(skip(self))]
    pub async fn clear(&self, user_id: &str) -> Result<bool, ConversationError> {
        let mut conversations = self.conversations.write().await;
        let removed = conversations.remove(user_id).is_some();

        if removed {
            info!("Cleared conversation for {}", user_id);
        }

        Ok(removed)
    }

    /// Convert conversation to OpenAI messages format.
    pub async fn to_openai_messages(
        &self,
        user_id: &str,
        system_prompt: Option<&str>,
    ) -> Result<Vec<OpenAiMessage>, ConversationError> {
        let conv = self.get(user_id).await?;
        let mut messages = Vec::new();

        // Add system prompt
        let prompt = system_prompt
            .map(String::from)
            .or_else(|| conv.as_ref().and_then(|c| c.system_prompt.clone()));

        if let Some(p) = prompt {
            messages.push(OpenAiMessage {
                role: "system".into(),
                content: Some(p),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        // Add conversation history
        if let Some(conv) = conv {
            for msg in conv.messages {
                messages.push(OpenAiMessage {
                    role: msg.role,
                    content: msg.content,
                    tool_calls: msg.tool_calls,
                    tool_call_id: msg.tool_call_id,
                });
            }
        }

        Ok(messages)
    }

    /// Get message count for a user.
    pub async fn message_count(&self, user_id: &str) -> Result<usize, ConversationError> {
        Ok(self
            .get(user_id)
            .await?
            .map(|c| c.messages.len())
            .unwrap_or(0))
    }

    /// Get total number of active conversations.
    pub async fn conversation_count(&self) -> usize {
        let conversations = self.conversations.read().await;
        let now = std::time::Instant::now();
        conversations
            .values()
            .filter(|entry| entry.expires_at > now)
            .count()
    }

    /// Get messages from today (UTC) for a given conversation.
    pub async fn get_today_messages(&self, user_id: &str) -> Result<Vec<StoredMessage>, ConversationError> {
        let today = chrono::Utc::now().date_naive();
        Ok(self
            .get(user_id)
            .await?
            .map(|conv| {
                conv.messages
                    .into_iter()
                    .filter(|m| m.timestamp.date_naive() == today)
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Health check - always returns true for in-memory store.
    pub async fn health_check(&self) -> bool {
        true
    }

    /// Add an assistant message with tool calls.
    #[instrument(skip(self, tool_calls))]
    pub async fn add_assistant_with_tools(
        &self,
        user_id: &str,
        content: Option<&str>,
        tool_calls: &[crate::types::StoredToolCall],
    ) -> Result<Conversation, ConversationError> {
        let message = StoredMessage::with_tool_calls(
            "assistant",
            content.map(String::from),
            tool_calls.to_vec(),
        );

        self.add_message_internal(user_id, message).await
    }

    /// Add a tool result message.
    #[instrument(skip(self, content))]
    pub async fn add_tool_result(
        &self,
        user_id: &str,
        tool_call_id: &str,
        content: &str,
    ) -> Result<Conversation, ConversationError> {
        let message = StoredMessage::tool_result(tool_call_id, content);
        self.add_message_internal(user_id, message).await
    }

    /// Internal method to add a pre-constructed message.
    async fn add_message_internal(
        &self,
        user_id: &str,
        message: StoredMessage,
    ) -> Result<Conversation, ConversationError> {
        let mut conversations = self.conversations.write().await;
        let now = std::time::Instant::now();
        let expires_at = now + self.ttl;

        let entry = conversations
            .entry(user_id.to_string())
            .or_insert_with(|| ConversationEntry {
                conversation: Conversation::new(user_id, None),
                expires_at,
            });

        // Update expiration on activity
        entry.expires_at = expires_at;

        // Add the message
        entry.conversation.messages.push(message);
        entry.conversation.updated_at = chrono::Utc::now();

        // Trim old messages
        entry.conversation.trim(self.max_messages);

        debug!(
            "Added message for {} (total: {})",
            user_id,
            entry.conversation.messages.len()
        );

        Ok(entry.conversation.clone())
    }
}
