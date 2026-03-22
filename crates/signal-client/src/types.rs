//! Signal API types.

use serde::{Deserialize, Serialize};

/// Incoming Signal message.
#[derive(Debug, Clone, Deserialize)]
pub struct IncomingMessage {
    pub envelope: Envelope,
    pub account: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Envelope {
    pub source: String,
    #[serde(rename = "sourceNumber")]
    pub source_number: Option<String>,
    #[serde(rename = "sourceName")]
    pub source_name: Option<String>,
    pub timestamp: i64,
    #[serde(rename = "dataMessage")]
    pub data_message: Option<DataMessage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DataMessage {
    pub message: Option<String>,
    pub timestamp: i64,
    #[serde(rename = "groupInfo")]
    pub group_info: Option<GroupInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GroupInfo {
    #[serde(rename = "groupId")]
    pub group_id: String,
}

/// Quote for replying to a specific message.
#[derive(Debug, Clone, Serialize)]
pub struct Quote {
    pub id: i64,
    pub author: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// Outgoing message request.
#[derive(Debug, Clone, Serialize)]
pub struct SendMessageRequest {
    pub message: String,
    pub number: Option<String>,
    pub recipients: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote: Option<Quote>,
}

/// Send message response.
#[derive(Debug, Clone, Deserialize)]
pub struct SendMessageResponse {
    pub timestamp: Option<i64>,
}

/// Account information.
#[derive(Debug, Clone, Deserialize)]
pub struct Account {
    pub number: String,
    pub uuid: Option<String>,
    pub registered: bool,
}

/// Parsed message for bot processing.
#[derive(Debug, Clone)]
pub struct BotMessage {
    /// The phone number that sent the message.
    pub source: String,
    /// The message text.
    pub text: String,
    /// Message timestamp.
    pub timestamp: i64,
    /// Whether this is a group message.
    pub is_group: bool,
    /// Group ID if this is a group message.
    pub group_id: Option<String>,
    /// The bot's phone number that received this message.
    pub receiving_account: String,
}

impl BotMessage {
    /// Extract bot message from incoming envelope.
    pub fn from_incoming(msg: &IncomingMessage) -> Option<Self> {
        let data = msg.envelope.data_message.as_ref()?;
        let text = data.message.clone()?;

        Some(Self {
            source: msg.envelope.source.clone(),
            text,
            timestamp: msg.envelope.timestamp,
            is_group: data.group_info.is_some(),
            group_id: data.group_info.as_ref().map(|g| g.group_id.clone()),
            receiving_account: msg.account.clone(),
        })
    }

    /// Get the reply target (group ID or source number).
    pub fn reply_target(&self) -> &str {
        self.group_id.as_deref().unwrap_or(&self.source)
    }
}
