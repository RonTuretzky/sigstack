//! Signal HTTP client.

use crate::error::SignalError;
use crate::types::*;
use reqwest::Client;
use std::time::Duration;
use tracing::{debug, instrument, warn};
use urlencoding::encode;

/// Signal CLI REST API client.
///
/// Supports multi-account operations - can send/receive for any registered account.
#[derive(Clone)]
pub struct SignalClient {
    client: Client,
    base_url: String,
}

impl SignalClient {
    /// Create a new Signal client.
    pub fn new(base_url: impl Into<String>) -> Result<Self, SignalError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;

        Ok(Self {
            client,
            base_url: base_url.into(),
        })
    }

    /// List all registered accounts.
    #[instrument(skip(self))]
    pub async fn list_accounts(&self) -> Result<Vec<String>, SignalError> {
        let response = self
            .client
            .get(format!("{}/v1/accounts", self.base_url))
            .send()
            .await?;

        if !response.status().is_success() {
            let msg = response.text().await.unwrap_or_default();
            return Err(SignalError::Api(msg));
        }

        let accounts: Vec<String> = response.json().await?;
        debug!("Found {} registered accounts", accounts.len());
        Ok(accounts)
    }

    /// Check if the Signal API is healthy.
    pub async fn health_check(&self) -> bool {
        self.client
            .get(format!("{}/v1/health", self.base_url))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    /// Get account information for a specific phone number.
    #[instrument(skip(self))]
    pub async fn get_account(&self, phone_number: &str) -> Result<Account, SignalError> {
        let encoded_number = encode(phone_number);
        let response = self
            .client
            .get(format!("{}/v1/accounts/{}", self.base_url, encoded_number))
            .send()
            .await?;

        if !response.status().is_success() {
            let msg = response.text().await.unwrap_or_default();
            return Err(SignalError::Api(msg));
        }

        Ok(response.json().await?)
    }

    /// Receive pending messages for a specific phone number.
    #[instrument(skip(self))]
    pub async fn receive(&self, phone_number: &str) -> Result<Vec<IncomingMessage>, SignalError> {
        let encoded_number = encode(phone_number);
        let response = self
            .client
            .get(format!(
                "{}/v1/receive/{}",
                self.base_url, encoded_number
            ))
            .send()
            .await?;

        if !response.status().is_success() {
            let msg = response.text().await.unwrap_or_default();
            return Err(SignalError::Api(msg));
        }

        let messages: Vec<IncomingMessage> = response.json().await?;
        debug!("Received {} messages for {}", messages.len(), phone_number);
        Ok(messages)
    }

    /// Send a message from a specific account to a recipient.
    #[instrument(skip(self, message))]
    pub async fn send(
        &self,
        from_number: &str,
        recipient: &str,
        message: &str,
    ) -> Result<(), SignalError> {
        let request = SendMessageRequest {
            message: message.to_string(),
            number: Some(from_number.to_string()),
            recipients: Some(vec![recipient.to_string()]),
            quote: None,
        };

        let response = self
            .client
            .post(format!("{}/v2/send", self.base_url))
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let msg = response.text().await.unwrap_or_default();
            warn!("Send failed: {}", msg);
            return Err(SignalError::SendFailed(msg));
        }

        debug!("Sent message from {} to {}", from_number, recipient);
        Ok(())
    }

    /// Send a message with a quote (reply to a specific message).
    #[instrument(skip(self, message, quote))]
    pub async fn send_with_quote(
        &self,
        from_number: &str,
        recipient: &str,
        message: &str,
        quote: Quote,
    ) -> Result<(), SignalError> {
        let request = SendMessageRequest {
            message: message.to_string(),
            number: Some(from_number.to_string()),
            recipients: Some(vec![recipient.to_string()]),
            quote: Some(quote),
        };

        let response = self
            .client
            .post(format!("{}/v2/send", self.base_url))
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let msg = response.text().await.unwrap_or_default();
            warn!("Send with quote failed: {}", msg);
            return Err(SignalError::SendFailed(msg));
        }

        debug!("Sent quoted message from {} to {}", from_number, recipient);
        Ok(())
    }

    /// Reply to a message (handles both direct and group messages).
    /// Uses the receiving account to send the reply.
    pub async fn reply(&self, original: &BotMessage, message: &str) -> Result<(), SignalError> {
        self.send(&original.receiving_account, original.reply_target(), message)
            .await
    }
}
