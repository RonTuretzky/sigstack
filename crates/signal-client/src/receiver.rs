//! Message receiver with WebSocket streaming and HTTP polling fallback.

use crate::client::SignalClient;
use crate::types::*;
use futures_util::StreamExt;
use std::time::Duration;
use tokio::time::sleep;
use tokio_stream::Stream;
use tracing::{debug, error, info, warn};

/// Message receiver that connects to Signal CLI for new messages.
///
/// Supports two modes:
/// - **WebSocket** (json-rpc mode): Persistent connection, messages pushed in real-time.
///   No file lock contention from polling.
/// - **HTTP polling** (normal mode): Polls `/v1/receive` periodically. Causes file lock
///   contention with concurrent requests.
pub struct MessageReceiver {
    client: SignalClient,
    poll_interval: Duration,
    /// How often to refresh the account list
    account_refresh_interval: Duration,
    /// Base URL for Signal CLI (used to construct WebSocket URL)
    base_url: String,
    /// Whether to use WebSocket mode
    use_websocket: bool,
}

impl MessageReceiver {
    /// Create a new message receiver with HTTP polling (normal mode).
    pub fn new(client: SignalClient, poll_interval: Duration) -> Self {
        Self {
            client,
            poll_interval,
            account_refresh_interval: Duration::from_secs(300),
            base_url: String::new(),
            use_websocket: false,
        }
    }

    /// Create a new message receiver with WebSocket streaming (json-rpc mode).
    pub fn new_websocket(client: SignalClient, base_url: impl Into<String>) -> Self {
        Self {
            client,
            poll_interval: Duration::from_secs(5),
            account_refresh_interval: Duration::from_secs(300),
            base_url: base_url.into(),
            use_websocket: true,
        }
    }

    /// Start receiving messages as an async stream.
    pub fn stream(self) -> std::pin::Pin<Box<dyn Stream<Item = BotMessage> + Send>> {
        if self.use_websocket {
            Box::pin(self.websocket_stream())
        } else {
            Box::pin(self.polling_stream())
        }
    }

    fn websocket_stream(self) -> impl Stream<Item = BotMessage> {
        async_stream::stream! {
            let mut accounts: Vec<String> = Vec::new();
            let mut last_account_refresh = std::time::Instant::now();

            loop {
                // Refresh account list
                if accounts.is_empty()
                    || last_account_refresh.elapsed() >= self.account_refresh_interval
                {
                    match self.client.list_accounts().await {
                        Ok(new_accounts) => {
                            if new_accounts != accounts {
                                info!("WebSocket receiver: {} accounts: {:?}", new_accounts.len(), new_accounts);
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
                        }
                    }
                }

                if accounts.is_empty() {
                    warn!("No registered accounts found, waiting 60s...");
                    sleep(Duration::from_secs(60)).await;
                    continue;
                }

                // Connect WebSocket for each account
                for account in &accounts {
                    let ws_url = build_ws_url(&self.base_url, account);
                    info!("Connecting WebSocket for {} at {}", account, ws_url);

                    match tokio_tungstenite::connect_async(&ws_url).await {
                        Ok((ws_stream, _response)) => {
                            info!("WebSocket connected for {}", account);
                            let (_, mut read) = ws_stream.split();

                            // Read messages until disconnect
                            while let Some(msg_result) = read.next().await {
                                // Check if we need to refresh accounts
                                if last_account_refresh.elapsed() >= self.account_refresh_interval {
                                    break; // Break inner loop to refresh accounts
                                }

                                match msg_result {
                                    Ok(msg) => {
                                        if let tokio_tungstenite::tungstenite::Message::Text(text) = msg {
                                            match serde_json::from_str::<IncomingMessage>(&text) {
                                                Ok(incoming) => {
                                                    if let Some(bot_msg) = BotMessage::from_incoming(&incoming) {
                                                        debug!(
                                                            "WS received on {}: text_len={} from {}",
                                                            account,
                                                            bot_msg.text.len(),
                                                            bot_msg.source
                                                        );
                                                        yield bot_msg;
                                                    }
                                                }
                                                Err(e) => {
                                                    debug!("Non-message WebSocket frame: {}", e);
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        warn!("WebSocket error for {}: {}", account, e);
                                        break;
                                    }
                                }
                            }

                            info!("WebSocket disconnected for {}, reconnecting...", account);
                        }
                        Err(e) => {
                            error!("Failed to connect WebSocket for {}: {}", account, e);
                        }
                    }
                }

                // Brief pause before reconnecting
                sleep(Duration::from_secs(5)).await;
            }
        }
    }

    fn polling_stream(self) -> impl Stream<Item = BotMessage> {
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
                                        "Received on {}: text_len={} from {}",
                                        account,
                                        bot_msg.text.len(),
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

/// Convert an HTTP base URL to a WebSocket URL for the receive endpoint.
fn build_ws_url(base_url: &str, account: &str) -> String {
    let ws_base = base_url
        .replace("http://", "ws://")
        .replace("https://", "wss://");
    let encoded = urlencoding::encode(account);
    format!("{}/v1/receive/{}", ws_base, encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_ws_url_http() {
        let url = build_ws_url("http://signal-api:8080", "+12013793585");
        assert_eq!(url, "ws://signal-api:8080/v1/receive/%2B12013793585");
    }

    #[test]
    fn test_build_ws_url_https() {
        let url = build_ws_url("https://example.com", "+12013793585");
        assert_eq!(url, "wss://example.com/v1/receive/%2B12013793585");
    }
}
