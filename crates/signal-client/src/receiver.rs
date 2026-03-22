//! Message receiver with polling.

use crate::client::SignalClient;
use crate::types::*;
use std::time::Duration;
use tokio::time::sleep;
use tokio_stream::Stream;
use tracing::{debug, error, info, warn};

/// Message receiver that polls all registered accounts for new messages.
pub struct MessageReceiver {
    client: SignalClient,
    poll_interval: Duration,
    /// How often to refresh the account list
    account_refresh_interval: Duration,
}

impl MessageReceiver {
    /// Create a new message receiver.
    pub fn new(client: SignalClient, poll_interval: Duration) -> Self {
        Self {
            client,
            poll_interval,
            // Refresh account list every 5 minutes
            account_refresh_interval: Duration::from_secs(300),
        }
    }

    /// Start receiving messages from all registered accounts as an async stream.
    pub fn stream(self) -> impl Stream<Item = BotMessage> {
        async_stream::stream! {
            let mut accounts: Vec<String> = Vec::new();
            let mut last_account_refresh = std::time::Instant::now();
            loop {
                // Refresh account list periodically or on first run
                if accounts.is_empty()
                    || last_account_refresh.elapsed() >= self.account_refresh_interval
                {
                    match self.client.list_accounts().await {
                        Ok(new_accounts) => {
                            if new_accounts != accounts {
                                info!("Polling {} accounts: {:?}", new_accounts.len(), new_accounts);
                            }
                            accounts = new_accounts;
                            last_account_refresh = std::time::Instant::now();
                        }
                        Err(e) => {
                            error!("Failed to list accounts: {}", e);
                            if accounts.is_empty() {
                                sleep(Duration::from_secs(60)).await;
                                continue;
                            }
                            // Continue with cached accounts
                        }
                    }
                }

                if accounts.is_empty() {
                    warn!("No registered accounts found, waiting 60s...");
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }

                // Poll each account for messages
                for account in &accounts {
                    match self.client.receive(account).await {
                        Ok(messages) => {
                            for msg in messages {
                                if let Some(bot_msg) = BotMessage::from_incoming(&msg) {
                                    debug!(
                                        "Received on {}: '{}' from {}",
                                        account,
                                        &bot_msg.text.chars().take(50).collect::<String>(),
                                        bot_msg.source
                                    );
                                    yield bot_msg;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Receive error for {}: {}", account, e);
                            // Continue to next account
                        }
                    }
                }

                sleep(self.poll_interval).await;
            }
        }
    }
}
