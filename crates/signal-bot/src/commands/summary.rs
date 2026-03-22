//! Summary command - generates a daily conversation summary using NEAR AI.

use crate::commands::CommandHandler;
use crate::error::AppResult;
use async_trait::async_trait;
use conversation_store::ConversationStore;
use near_ai_client::{Message, NearAiClient};
use signal_client::BotMessage;
use std::sync::Arc;
use tracing::{debug, error, instrument};

pub struct SummaryHandler {
    near_ai: Arc<NearAiClient>,
    conversations: Arc<ConversationStore>,
}

impl SummaryHandler {
    pub fn new(
        near_ai: Arc<NearAiClient>,
        conversations: Arc<ConversationStore>,
    ) -> Self {
        Self {
            near_ai,
            conversations,
        }
    }

    /// Format conversation messages into a transcript for summarization.
    fn format_transcript(messages: &[conversation_store::StoredMessage]) -> String {
        let mut transcript = String::new();
        for msg in messages {
            if msg.role == "tool" {
                continue;
            }
            let content = match &msg.content {
                Some(c) => c.as_str(),
                None => continue,
            };
            let time = msg.timestamp.format("%H:%M UTC");
            let role_label = match msg.role.as_str() {
                "user" => "User",
                "assistant" => "Assistant",
                _ => &msg.role,
            };
            transcript.push_str(&format!("[{}] {}: {}\n", time, role_label, content));
        }
        transcript
    }
}

#[async_trait]
impl CommandHandler for SummaryHandler {
    fn trigger(&self) -> Option<&str> {
        Some("!summary")
    }

    #[instrument(skip(self, message), fields(user = %message.source))]
    async fn execute(&self, message: &BotMessage) -> AppResult<String> {
        let conversation_id = message.reply_target();

        let today_messages = self
            .conversations
            .get_today_messages(&conversation_id)
            .await?;

        if today_messages.is_empty() {
            return Ok("No messages found for today. Start a conversation first!".into());
        }

        let relevant: Vec<_> = today_messages
            .iter()
            .filter(|m| (m.role == "user" || m.role == "assistant") && m.content.is_some())
            .collect();

        if relevant.is_empty() {
            return Ok("No conversation messages found for today.".into());
        }

        let transcript = Self::format_transcript(&today_messages);
        debug!(
            "Summarizing {} messages ({} chars) for {}",
            relevant.len(),
            transcript.len(),
            &conversation_id[..conversation_id.len().min(12)]
        );

        let messages = vec![
            Message::system(
                "You are a concise summarizer. Given a conversation transcript from today, \
                 produce a clear summary of the key topics discussed, decisions made, questions asked, \
                 and any important information exchanged. Keep it brief and organized with bullet points. \
                 Do not include timestamps in the summary.",
            ),
            Message::user(format!(
                "Please summarize today's conversation:\n\n{}",
                transcript
            )),
        ];

        match self.near_ai.chat(messages, Some(0.3), None).await {
            Ok(summary) => {
                let header = format!(
                    "**Today's Conversation Summary** ({} messages)\n\n",
                    relevant.len()
                );
                Ok(format!("{}{}", header, summary))
            }
            Err(e) => {
                error!("Summary generation failed: {}", e);
                Ok(format!(
                    "Failed to generate summary. Please try again. ({})",
                    e
                ))
            }
        }
    }
}
