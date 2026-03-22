//! Kanban command - scans conversation history and generates a Mermaid Kanban board.

use crate::commands::CommandHandler;
use crate::error::AppResult;
use async_trait::async_trait;
use conversation_store::ConversationStore;
use near_ai_client::{Message, NearAiClient};
use signal_client::BotMessage;
use std::sync::Arc;
use tracing::{debug, error, info};

const KANBAN_SYSTEM_PROMPT: &str = r#"You are a task extraction assistant. Analyze the conversation history and extract all tasks, action items, TODOs, and commitments mentioned by any participant.

For each task, determine its status based on the conversation context:
- "Todo" if the task was mentioned but not started
- "In Progress" if someone indicated they are working on it
- "Done" if someone confirmed completion
- "Blocked" if there is an impediment mentioned

Output ONLY a valid Mermaid kanban diagram using this exact format. Do not include any other text, explanation, or markdown code fences.

---
config:
  kanban:
    ticketBaseUrl: ''
---
kanban
  Todo
    id1[Task description]
    id2[Task description]
  In-Progress
    id3[Task description]
  Done
    id4[Task description]
  Blocked
    id5[Task description]

Rules:
- Use short, clear task descriptions (under 60 chars each)
- Assign unique ids (t1, t2, t3, etc.)
- Omit empty columns entirely
- If no tasks are found, output a kanban with a single Todo item: t1[No tasks found in conversation]
- Do NOT wrap output in code fences or add any explanation"#;

pub struct KanbanHandler {
    near_ai: Arc<NearAiClient>,
    conversations: Arc<ConversationStore>,
}

impl KanbanHandler {
    pub fn new(near_ai: Arc<NearAiClient>, conversations: Arc<ConversationStore>) -> Self {
        Self {
            near_ai,
            conversations,
        }
    }
}

#[async_trait]
impl CommandHandler for KanbanHandler {
    fn trigger(&self) -> Option<&str> {
        Some("!kanban")
    }

    async fn execute(&self, message: &BotMessage) -> AppResult<String> {
        let conversation_id = message.reply_target();
        info!("Generating kanban board for conversation {}", &conversation_id[..conversation_id.len().min(12)]);

        // Get conversation history
        let conversation = self.conversations.get(conversation_id).await?;

        let history_text = match conversation {
            Some(conv) if !conv.messages.is_empty() => {
                let mut lines = Vec::new();
                for msg in &conv.messages {
                    if let Some(ref content) = msg.content {
                        let role = match msg.role.as_str() {
                            "user" => "User",
                            "assistant" => "Assistant",
                            _ => continue, // skip tool/system messages
                        };
                        lines.push(format!("{}: {}", role, content));
                    }
                }
                if lines.is_empty() {
                    return Ok("No conversation history found. Start chatting first, then use `!kanban` to generate a task board.".into());
                }
                lines.join("\n")
            }
            _ => {
                return Ok("No conversation history found. Start chatting first, then use `!kanban` to generate a task board.".into());
            }
        };

        debug!("Conversation history for kanban: {} chars", history_text.len());

        // Send to LLM for task extraction
        let messages = vec![
            Message::system(KANBAN_SYSTEM_PROMPT),
            Message::user(format!("Here is the conversation to analyze:\n\n{}", history_text)),
        ];

        match self.near_ai.chat(messages, Some(0.3), None).await {
            Ok(mermaid) => {
                // Clean up: strip code fences if the model added them
                let cleaned = mermaid.trim();
                let cleaned = cleaned
                    .strip_prefix("```mermaid")
                    .or_else(|| cleaned.strip_prefix("```"))
                    .unwrap_or(cleaned);
                let cleaned = cleaned.strip_suffix("```").unwrap_or(cleaned).trim();

                info!("Generated kanban board ({} chars)", cleaned.len());
                Ok(cleaned.to_string())
            }
            Err(e) => {
                error!("Failed to generate kanban: {}", e);
                Ok("Failed to generate kanban board. Please try again.".into())
            }
        }
    }
}
