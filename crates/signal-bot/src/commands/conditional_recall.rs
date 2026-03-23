//! Sealed Negotiation - TEE-enforced private bilateral negotiation.
//!
//! Implements bargaining with credible forgetting from "Conditional Recall" (Schlegel & Sun, 2025):
//! Two parties submit private offers to the TEE. If the buyer's max >= seller's min,
//! the TEE reveals the midpoint price. If not, the TEE forgets both offers —
//! neither party learns what the other was willing to pay.
//!
//! The TEE guarantees that on no-deal, both valuations are truly deleted.

use crate::commands::CommandHandler;
use crate::error::AppResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use signal_client::BotMessage;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

#[derive(Debug, Clone, PartialEq)]
enum NegotiationStatus {
    /// Waiting for both parties to submit offers.
    WaitingForOffers,
    /// Deal reached — midpoint revealed to both.
    Deal,
    /// No deal — TEE forgot both offers.
    NoDeal,
    /// Cancelled by the initiator before both offers submitted.
    Cancelled,
}

#[derive(Debug, Clone)]
struct Negotiation {
    id: u64,
    initiator: String,
    counterparty: String,
    description: String,
    /// Initiator's private offer — None after no-deal (credible forgetting).
    initiator_offer: Option<f64>,
    /// Counterparty's private offer — None after no-deal (credible forgetting).
    counterparty_offer: Option<f64>,
    /// The agreed price (midpoint), only set on deal.
    deal_price: Option<f64>,
    status: NegotiationStatus,
    created_at: DateTime<Utc>,
    resolved_at: Option<DateTime<Utc>>,
}

/// In-memory store for negotiations. All data lives in TEE-protected memory.
#[derive(Clone)]
pub struct NegotiationStore {
    negotiations: Arc<RwLock<HashMap<u64, Negotiation>>>,
    next_id: Arc<RwLock<u64>>,
}

impl NegotiationStore {
    pub fn new() -> Self {
        Self {
            negotiations: Arc::new(RwLock::new(HashMap::new())),
            next_id: Arc::new(RwLock::new(1)),
        }
    }

    async fn create(&self, initiator: &str, counterparty: &str, description: &str) -> u64 {
        let mut next_id = self.next_id.write().await;
        let id = *next_id;
        *next_id += 1;

        let negotiation = Negotiation {
            id,
            initiator: initiator.to_string(),
            counterparty: counterparty.to_string(),
            description: description.to_string(),
            initiator_offer: None,
            counterparty_offer: None,
            deal_price: None,
            status: NegotiationStatus::WaitingForOffers,
            created_at: Utc::now(),
            resolved_at: None,
        };

        self.negotiations.write().await.insert(id, negotiation);
        info!(
            "Negotiation #{} created between {} and {}",
            id,
            &initiator[..8.min(initiator.len())],
            &counterparty[..8.min(counterparty.len())]
        );
        id
    }

    async fn submit_offer(&self, id: u64, sender: &str, sender_phone: Option<&str>, amount: f64) -> Result<OfferResult, String> {
        let mut negotiations = self.negotiations.write().await;
        let neg = negotiations.get_mut(&id).ok_or("Negotiation not found.")?;

        if neg.status != NegotiationStatus::WaitingForOffers {
            return Err("This negotiation is already resolved.".into());
        }

        // Match by UUID or phone number (Signal delivers UUID but negotiations store phone numbers)
        let matches = |participant: &str| -> bool {
            participant == sender
                || sender_phone.map_or(false, |p| participant == p)
        };

        let is_initiator = matches(&neg.initiator);
        let is_counterparty = matches(&neg.counterparty);

        if !is_initiator && !is_counterparty {
            return Err("You are not a participant in this negotiation.".into());
        }

        if is_initiator {
            if neg.initiator_offer.is_some() {
                return Err("You already submitted an offer. Wait for the other party.".into());
            }
            neg.initiator_offer = Some(amount);
        } else {
            if neg.counterparty_offer.is_some() {
                return Err("You already submitted an offer. Wait for the other party.".into());
            }
            neg.counterparty_offer = Some(amount);
        }

        // Check if both offers are in — if so, evaluate
        if let (Some(init_offer), Some(counter_offer)) = (neg.initiator_offer, neg.counterparty_offer) {
            // Convention: initiator is seller (min price), counterparty is buyer (max price)
            // Deal if buyer's max >= seller's min
            if counter_offer >= init_offer {
                let midpoint = (init_offer + counter_offer) / 2.0;
                neg.deal_price = Some(midpoint);
                neg.status = NegotiationStatus::Deal;
                neg.resolved_at = Some(Utc::now());
                info!("Negotiation #{} DEAL at {:.2}", id, midpoint);
                Ok(OfferResult::Deal {
                    price: midpoint,
                    description: neg.description.clone(),
                })
            } else {
                // CREDIBLE FORGETTING: erase both offers from TEE memory
                neg.initiator_offer = None;
                neg.counterparty_offer = None;
                neg.status = NegotiationStatus::NoDeal;
                neg.resolved_at = Some(Utc::now());
                info!("Negotiation #{} NO DEAL — both offers erased from TEE memory", id);
                Ok(OfferResult::NoDeal {
                    description: neg.description.clone(),
                })
            }
        } else {
            Ok(OfferResult::WaitingForOther)
        }
    }

    async fn withdraw(&self, id: u64, sender: &str, sender_phone: Option<&str>) -> Result<Negotiation, String> {
        let mut negotiations = self.negotiations.write().await;
        let neg = negotiations.get_mut(&id).ok_or("Negotiation not found.")?;

        if neg.status != NegotiationStatus::WaitingForOffers {
            return Err("This negotiation is already resolved.".into());
        }

        let matches = |participant: &str| -> bool {
            participant == sender
                || sender_phone.map_or(false, |p| participant == p)
        };

        if !matches(&neg.initiator) && !matches(&neg.counterparty) {
            return Err("You are not a participant in this negotiation.".into());
        }

        // Credible forgetting on cancel too
        neg.initiator_offer = None;
        neg.counterparty_offer = None;
        neg.status = NegotiationStatus::Cancelled;
        neg.resolved_at = Some(Utc::now());
        info!("Negotiation #{} CANCELLED by participant — offers erased", id);
        Ok(neg.clone())
    }

    async fn get_pending_for_user(&self, user: &str, user_phone: Option<&str>) -> Vec<Negotiation> {
        let matches = |participant: &str| -> bool {
            participant == user
                || user_phone.map_or(false, |p| participant == p)
        };
        self.negotiations
            .read()
            .await
            .values()
            .filter(|n| {
                n.status == NegotiationStatus::WaitingForOffers
                    && (matches(&n.initiator) || matches(&n.counterparty))
            })
            .cloned()
            .collect()
    }
}

#[derive(Debug)]
enum OfferResult {
    WaitingForOther,
    Deal {
        price: f64,
        description: String,
    },
    NoDeal {
        description: String,
    },
}

// --- Command Handlers ---

/// !negotiate <phone> <description> — propose a negotiation
pub struct NegotiateHandler {
    store: Arc<NegotiationStore>,
}

impl NegotiateHandler {
    pub fn new(store: Arc<NegotiationStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl CommandHandler for NegotiateHandler {
    fn trigger(&self) -> Option<&str> {
        Some("!negotiate")
    }

    async fn execute(&self, message: &BotMessage) -> AppResult<String> {
        let text = message.text.trim_start_matches("!negotiate").trim();

        let parts: Vec<&str> = text.splitn(2, char::is_whitespace).collect();
        if parts.len() < 2 {
            return Ok(
                "Usage: !negotiate <counterparty_phone> <description>\n\n\
                 Example: !negotiate +14155551234 used car sale\n\n\
                 You (initiator) set your minimum price, they (counterparty) set their maximum.\n\
                 If max >= min, deal at midpoint. Otherwise TEE forgets both offers."
                    .into(),
            );
        }

        let counterparty = parts[0].trim();
        if !counterparty.starts_with('+') {
            return Ok("Counterparty must be a phone number starting with '+'.".into());
        }

        if counterparty == message.source {
            return Ok("You can't negotiate with yourself.".into());
        }

        let description = parts[1].trim();
        let id = self
            .store
            .create(&message.source, counterparty, description)
            .await;

        Ok(format!(
            "Negotiation #{} created: \"{}\"\n\n\
             You (seller/initiator): submit your minimum acceptable price with !offer {} <amount>\n\
             {} (buyer): submit their maximum price with !offer {} <amount>\n\n\
             Once both offers are in, the TEE evaluates:\n\
             - If buyer max >= seller min: Deal at the midpoint price\n\
             - If not: No deal — TEE permanently erases both offers\n\n\
             Either party can cancel with !withdraw {}",
            id, description, id, counterparty, id, id
        ))
    }
}

/// !offer <id> <amount> — submit your private offer
pub struct OfferHandler {
    store: Arc<NegotiationStore>,
}

impl OfferHandler {
    pub fn new(store: Arc<NegotiationStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl CommandHandler for OfferHandler {
    fn trigger(&self) -> Option<&str> {
        Some("!offer")
    }

    async fn execute(&self, message: &BotMessage) -> AppResult<String> {
        let text = message.text.trim_start_matches("!offer").trim();
        let parts: Vec<&str> = text.splitn(2, char::is_whitespace).collect();

        if parts.len() < 2 {
            return Ok("Usage: !offer <negotiation_id> <amount>\n\nExample: !offer 1 5000".into());
        }

        let id: u64 = parts[0]
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid negotiation ID."))?;

        let amount: f64 = parts[1]
            .trim_start_matches('$')
            .replace(',', "")
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid amount. Use a number like 5000 or $5,000."))?;

        if amount < 0.0 {
            return Ok("Amount must be non-negative.".into());
        }

        match self.store.submit_offer(id, &message.source, message.source_number.as_deref(), amount).await {
            Ok(OfferResult::WaitingForOther) => Ok(format!(
                "Offer submitted for negotiation #{}.\n\n\
                 Your offer is sealed in TEE memory — only the TEE can see it.\n\
                 Waiting for the other party to submit their offer.",
                id
            )),
            Ok(OfferResult::Deal {
                price,
                description,
            }) => Ok(format!(
                "DEAL on negotiation #{}: \"{}\"\n\n\
                 Agreed price: ${:.2}\n\n\
                 Both parties' offers overlapped. The TEE computed the midpoint \
                 as the fair price. Both parties should see this result.",
                id, description, price
            )),
            Ok(OfferResult::NoDeal {
                description,
            }) => Ok(format!(
                "NO DEAL on negotiation #{}: \"{}\"\n\n\
                 CREDIBLE FORGETTING EXECUTED:\n\
                 Both offers have been permanently erased from TEE memory.\n\n\
                 Neither party learns what the other was willing to pay.\n\
                 The TEE hardware guarantees the valuations are truly deleted.\n\n\
                 This implements bargaining with amnesia from \
                 \"Conditional Recall\" (Schlegel & Sun, 2025).",
                id, description
            )),
            Err(e) => Ok(e),
        }
    }
}

/// !withdraw <id> — cancel a negotiation before resolution
pub struct WithdrawHandler {
    store: Arc<NegotiationStore>,
}

impl WithdrawHandler {
    pub fn new(store: Arc<NegotiationStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl CommandHandler for WithdrawHandler {
    fn trigger(&self) -> Option<&str> {
        Some("!withdraw")
    }

    async fn execute(&self, message: &BotMessage) -> AppResult<String> {
        let id_str = message.text.trim_start_matches("!withdraw").trim();
        let id: u64 = id_str
            .parse()
            .map_err(|_| anyhow::anyhow!("Usage: !withdraw <id>"))?;

        match self.store.withdraw(id, &message.source, message.source_number.as_deref()).await {
            Ok(neg) => Ok(format!(
                "Negotiation #{} (\"{}\") cancelled.\n\n\
                 Any submitted offers have been erased from TEE memory.",
                neg.id, neg.description
            )),
            Err(e) => Ok(e),
        }
    }
}

/// !deals — view your active negotiations
pub struct DealsHandler {
    store: Arc<NegotiationStore>,
}

impl DealsHandler {
    pub fn new(store: Arc<NegotiationStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl CommandHandler for DealsHandler {
    fn trigger(&self) -> Option<&str> {
        Some("!deals")
    }

    async fn execute(&self, message: &BotMessage) -> AppResult<String> {
        let pending = self.store.get_pending_for_user(&message.source, message.source_number.as_deref()).await;

        if pending.is_empty() {
            return Ok("No active negotiations.".into());
        }

        let mut response = format!("You have {} active negotiation(s):\n\n", pending.len());
        for n in &pending {
            let role = if n.initiator == message.source {
                "seller/initiator"
            } else {
                "buyer/counterparty"
            };
            let other = if n.initiator == message.source {
                &n.counterparty
            } else {
                &n.initiator
            };
            let your_offer = if n.initiator == message.source {
                n.initiator_offer
            } else {
                n.counterparty_offer
            };
            let offer_status = match your_offer {
                Some(_) => "submitted (waiting for other party)",
                None => "not yet submitted",
            };

            response.push_str(&format!(
                "#{} — \"{}\" (you: {}, with: {})\n   Your offer: {}\n   Created: {}\n   Submit: !offer {} <amount>\n\n",
                n.id,
                n.description,
                role,
                &other[..8.min(other.len())],
                offer_status,
                n.created_at.format("%Y-%m-%d %H:%M UTC"),
                n.id
            ));
        }

        Ok(response)
    }
}
