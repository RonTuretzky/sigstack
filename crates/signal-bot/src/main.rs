//! Signal AI Proxy Bot - Main entry point.

use signal_bot::commands::*;
use signal_bot::config::Config;
use signal_bot::error::AppResult;
use anyhow::Context;
use conversation_store::ConversationStore;
use dstack_client::DstackClient;
use near_ai_client::NearAiClient;
use signal_client::{MessageReceiver, SignalClient};
use std::sync::Arc;
use tokio::signal;
use tokio_stream::StreamExt;
use tools::{ToolRegistry, builtin::{CalculatorTool, WeatherTool, WebSearchTool}};
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use x402_payments::CreditStore;

/// Create and configure tool registry based on config.
fn create_tool_registry(config: &signal_bot::config::ToolsConfig) -> ToolRegistry {
    let mut registry = ToolRegistry::new();

    if !config.enabled {
        info!("Tools system disabled by configuration");
        return registry;
    }

    // Calculator - always available (no API key needed)
    if config.calculator.enabled {
        registry.register(Arc::new(CalculatorTool::new()));
        info!("Registered tool: calculate");
    }

    // Weather - always available (no API key needed)
    if config.weather.enabled {
        registry.register(Arc::new(WeatherTool::new()));
        info!("Registered tool: get_weather");
    }

    // Web search - requires API key
    if config.web_search.enabled {
        if let Some(api_key) = &config.web_search.api_key {
            let tool = WebSearchTool::new(api_key.clone())
                .with_max_results(config.web_search.max_results);
            registry.register(Arc::new(tool));
            info!("Registered tool: web_search (max_results: {})", config.web_search.max_results);
        } else {
            warn!("Web search tool enabled but TOOLS__WEB_SEARCH__API_KEY not set - skipping");
        }
    }

    let enabled_count = registry.list_enabled().len();
    info!("Tool registry ready with {} enabled tools", enabled_count);

    registry
}

#[tokio::main]
async fn main() -> AppResult<()> {
    // Load configuration
    let config = Config::load().context("Failed to load configuration")?;

    // Initialize logging
    init_logging(&config.bot.log_level);

    info!("Starting Signal AI Proxy Bot...");

    // Initialize clients
    let near_ai = Arc::new(
        NearAiClient::new(
            &config.near_ai.api_key,
            &config.near_ai.base_url,
            &config.near_ai.model,
            config.near_ai.timeout,
        )
        .context("Failed to create NEAR AI client")?,
    );

    let conversations = Arc::new(ConversationStore::new(
        config.conversation.max_messages,
        config.conversation.ttl,
    ));

    let dstack = Arc::new(DstackClient::new(&config.dstack.socket_path));

    let signal = Arc::new(
        SignalClient::new(&config.signal.service_url)
            .context("Failed to create Signal client")?,
    );

    // Create tool registry based on config
    let tool_registry = Arc::new(create_tool_registry(&config.tools));

    // Initialize payment system
    let credit_store = if config.payments.enabled {
        info!("Initializing payment system...");

        // Create separate DstackClient instances for payment system
        let payment_dstack = DstackClient::new(&config.dstack.socket_path);
        let server_dstack = DstackClient::new(&config.dstack.socket_path);

        let store = CreditStore::new(
            payment_dstack,
            config.payments.storage_path.clone(),
        )
        .await
        .context("Failed to initialize credit store")?;

        // Spawn payment HTTP server
        if let Some(handle) = x402_payments::spawn_payment_server(
            config.payments.clone(),
            server_dstack,
        )
        .await
        .context("Failed to start payment server")? {
            info!("Payment server started on port {}", config.payments.server_port);
            // Store handle to keep server running (we don't await it)
            tokio::spawn(async move {
                if let Err(e) = handle.await {
                    error!("Payment server error: {:?}", e);
                }
            });
        }

        Some(store)
    } else {
        info!("Payments disabled");
        None
    };

    // Health checks
    if near_ai.health_check().await {
        info!("NEAR AI healthy - Model: {}", config.near_ai.model);
    } else {
        warn!("NEAR AI health check failed - will retry on requests");
    }

    info!(
        "In-memory conversation store ready (max_messages={}, ttl={:?})",
        config.conversation.max_messages, config.conversation.ttl
    );

    if dstack.is_in_tee().await {
        if let Ok(info) = dstack.get_app_info().await {
            info!(
                "Running in TEE - App ID: {}",
                info.app_id.as_deref().unwrap_or("unknown")
            );
        }
    } else {
        warn!("Not running in TEE environment - attestation unavailable");
    }

    if !signal.health_check().await {
        error!("Signal API not reachable at {}", config.signal.service_url);
        return Err(anyhow::anyhow!("Signal API not reachable").into());
    }
    info!("Signal API healthy");

    // Create command handlers
    // Create ChatHandler with or without payment integration
    let chat_handler: Box<dyn CommandHandler> = if let Some(ref store) = credit_store {
        Box::new(ChatHandler::with_payments(
            near_ai.clone(),
            conversations.clone(),
            signal.clone(),
            tool_registry.clone(),
            config.bot.system_prompt.clone(),
            config.tools.max_tool_calls,
            config.bot.signal_username.clone(),
            config.bot.github_repo.clone(),
            store.clone(),
            config.payments.pricing.clone(),
        ))
    } else {
        Box::new(ChatHandler::new(
            near_ai.clone(),
            conversations.clone(),
            signal.clone(),
            tool_registry.clone(),
            config.bot.system_prompt.clone(),
            config.tools.max_tool_calls,
            config.bot.signal_username.clone(),
            config.bot.github_repo.clone(),
        ))
    };

    // Initialize negotiation store (TEE-protected in-memory)
    let negotiation_store = Arc::new(NegotiationStore::new());
    info!("Sealed negotiation store initialized (TEE-protected)");

    let mut handlers: Vec<Box<dyn CommandHandler>> = vec![
        chat_handler,
        Box::new(VerifyHandler::new(dstack.clone())),
        Box::new(ClearHandler::new(conversations.clone())),
        Box::new(HelpHandler::new()),
        Box::new(ModelsHandler::new(near_ai.clone())),
        Box::new(NegotiateHandler::new(negotiation_store.clone())),
        Box::new(OfferHandler::new(negotiation_store.clone())),
        Box::new(DealsHandler::new(negotiation_store.clone())),
        Box::new(WithdrawHandler::new(negotiation_store.clone())),
    ];

    // Add payment handlers if enabled
    if let Some(ref store) = credit_store {
        handlers.push(Box::new(BalanceHandler::new(store.clone())));
        handlers.push(Box::new(DepositHandler::new(config.payments.clone())));
        info!("Payment commands enabled: !balance, !deposit");
    }

    info!("Registered {} command handlers", handlers.len());
    info!("NEAR AI endpoint: {}", config.near_ai.base_url);
    info!("Listening for messages...");

    // Start message receiver
    let receiver = MessageReceiver::new((*signal).clone(), config.signal.poll_interval);
    let mut stream = Box::pin(receiver.stream());

    // Main message loop
    loop {
        tokio::select! {
            Some(message) = stream.next() => {
                // Find matching handler
                let handler = handlers
                    .iter()
                    .find(|h| h.matches(&message));

                if let Some(handler) = handler {
                    match handler.execute(&message).await {
                        Ok(response) => {
                            if let Err(e) = signal.reply(&message, &response).await {
                                error!("Failed to send reply: {}", e);
                            }
                        }
                        Err(e) => {
                            error!("Handler error: {}", e);
                            let _ = signal
                                .reply(&message, "Sorry, something went wrong.")
                                .await;
                        }
                    }
                }
            }
            _ = signal::ctrl_c() => {
                info!("Shutdown signal received");
                break;
            }
        }
    }

    info!("Shutting down...");
    Ok(())
}

fn init_logging(level: &str) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(level));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}
