//! Summary command - generates a daily conversation summary using a local LLM.

use crate::commands::CommandHandler;
use crate::error::AppResult;
use async_trait::async_trait;
use conversation_store::ConversationStore;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use signal_client::BotMessage;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, instrument};

/// Ollama-compatible chat request (OpenAI format).
#[derive(Debug, Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct OllamaMessage {
    role: String,
    content: String,
}

/// Ollama chat response.
#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    message: Option<OllamaResponseMessage>,
}

#[derive(Debug, Deserialize)]
struct OllamaResponseMessage {
    content: Option<String>,
}

pub struct SummaryHandler {
    conversations: Arc<ConversationStore>,
    ollama_url: String,
    ollama_model: String,
    http_client: Client,
}

impl SummaryHandler {
    pub fn new(
        conversations: Arc<ConversationStore>,
        ollama_url: String,
        ollama_model: String,
    ) -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to create HTTP client for Ollama");

        Self {
            conversations,
            ollama_url,
            ollama_model,
            http_client,
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

    /// Call the local Ollama instance to summarize the transcript.
    async fn summarize(&self, transcript: &str) -> Result<String, String> {
        let system_prompt = "You are a concise summarizer. Given a conversation transcript from today, \
            produce a clear summary of the key topics discussed, decisions made, questions asked, \
            and any important information exchanged. Keep it brief and organized with bullet points. \
            Do not include timestamps in the summary.";

        let request = OllamaChatRequest {
            model: self.ollama_model.clone(),
            messages: vec![
                OllamaMessage {
                    role: "system".into(),
                    content: system_prompt.into(),
                },
                OllamaMessage {
                    role: "user".into(),
                    content: format!(
                        "Please summarize today's conversation:\n\n{}",
                        transcript
                    ),
                },
            ],
            stream: false,
        };

        let url = format!("{}/api/chat", self.ollama_url);
        debug!("Sending summary request to Ollama at {}", url);

        let response = self
            .http_client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("Failed to reach local LLM: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Local LLM returned error {}: {}", status, body));
        }

        let chat_response: OllamaChatResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse LLM response: {}", e))?;

        chat_response
            .message
            .and_then(|m| m.content)
            .filter(|c| !c.is_empty())
            .ok_or_else(|| "Local LLM returned empty response".into())
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

        // Filter to only user/assistant messages with content
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

        match self.summarize(&transcript).await {
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
                    "Failed to generate summary: {}. \
                     Make sure the local LLM service is running.",
                    e
                ))
            }
        }
    }
}
