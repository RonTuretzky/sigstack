//! Help command - displays available commands.

use crate::commands::CommandHandler;
use crate::error::AppResult;
use async_trait::async_trait;
use signal_client::BotMessage;

pub struct HelpHandler;

impl HelpHandler {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HelpHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CommandHandler for HelpHandler {
    fn trigger(&self) -> Option<&str> {
        Some("!help")
    }

    async fn execute(&self, _message: &BotMessage) -> AppResult<String> {
        Ok(r#"**Signal AI** (Private & Verifiable)

Just send a message to chat with AI.

**Commands:**
- !verify <challenge> - Get TEE attestation with your challenge
- !clear - Clear conversation history
- !kanban - Generate a Mermaid Kanban board of tasks from this conversation
- !models - List available AI models
- !balance - Check your credit balance
- !deposit - Get deposit addresses for USDC
- !help - Show this message

**Verification:**
Use `!verify my-random-text` to get cryptographic proof this bot runs in a TEE. Your challenge is embedded in the TDX quote, proving the attestation was generated fresh for you.

**Payments:**
This bot uses prepaid credits. Deposit USDC on Base, NEAR, or Solana to add credits. Use `!balance` to check your balance and `!deposit` for deposit addresses.

**Privacy:**
Your messages are end-to-end encrypted via Signal, processed in a verified TEE (Intel TDX), and sent to NEAR AI Cloud's private inference (NVIDIA GPU TEE).

Neither the bot operator nor NEAR AI can read your messages."#
            .into())
    }
}
