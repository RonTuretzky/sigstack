//! Automatic translation interceptor for configured Signal groups.
//!
//! Runs before command handlers in the message loop. For messages in configured
//! groups, detects the language and sends a translated quoted reply.

use signal_client::{SignalClient, Quote};
use signal_client::BotMessage;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use translation::{LibreTranslateProvider, Translator, TranslatorConfig};

/// Pre-handler interceptor that auto-translates messages in configured groups.
pub struct TranslationInterceptor {
    translator: Translator,
    signal_client: Arc<SignalClient>,
    /// group_id -> (lang_a, lang_b)
    groups: HashMap<String, (String, String)>,
    /// If true, let the message continue to command handlers after translating.
    pub also_chat: bool,
}

impl TranslationInterceptor {
    pub fn new(
        libretranslate_url: &str,
        libretranslate_api_key: Option<String>,
        groups_config: &str,
        signal_client: Arc<SignalClient>,
        also_chat: bool,
    ) -> Self {
        let provider = Arc::new(LibreTranslateProvider::new(
            libretranslate_url.to_string(),
            libretranslate_api_key,
        ));

        let translator = Translator::new(provider, TranslatorConfig::default());
        let groups = parse_group_config(groups_config);

        info!("Translation interceptor initialized for {} groups", groups.len());
        for (group_id, (lang_a, lang_b)) in &groups {
            info!("  Group {}: {} <-> {}", group_id, lang_a, lang_b);
        }

        Self {
            translator,
            signal_client,
            groups,
            also_chat,
        }
    }

    /// Attempt to translate a message if it's in a configured group.
    ///
    /// Returns `true` if the message was in a configured translation group
    /// (whether or not a translation was actually sent). The caller uses this
    /// plus `also_chat` to decide whether to continue to command handlers.
    pub async fn try_translate(&self, message: &BotMessage) -> bool {
        // Only translate group messages
        if !message.is_group {
            return false;
        }

        // Allow commands through to handlers
        if message.text.starts_with('!') {
            return false;
        }

        // Check if this group is configured for translation
        let group_id = match &message.group_id {
            Some(id) => id,
            None => return false,
        };

        let (lang_a, lang_b) = match self.groups.get(group_id) {
            Some(pair) => pair,
            None => return false,
        };

        // Skip bot's own messages
        if message.source == message.receiving_account {
            return true;
        }

        debug!(
            "Translating message in group {} ({} <-> {}): {}",
            group_id,
            lang_a,
            lang_b,
            &message.text[..message.text.len().min(50)]
        );

        match self.translator.translate_if_needed(&message.text, lang_a, lang_b).await {
            Ok(Some(result)) => {
                let reply_text = format!("{} {}", result.flag_emoji, result.translated_text);
                let quote = Quote {
                    id: message.timestamp,
                    author: message.source.clone(),
                    text: Some(message.text.clone()),
                };

                if let Err(e) = self.signal_client.send_with_quote(
                    &message.receiving_account,
                    message.reply_target(),
                    &reply_text,
                    quote,
                ).await {
                    error!("Failed to send translation reply: {}", e);
                }
            }
            Ok(None) => {
                debug!("No translation needed (same language, too short, or low confidence)");
            }
            Err(e) => {
                warn!("Translation error: {}", e);
            }
        }

        true
    }
}

/// Parse group config string into a map of group_id -> (lang_a, lang_b).
///
/// Format: "group_id1:en:es,group_id2:en:fr"
fn parse_group_config(config: &str) -> HashMap<String, (String, String)> {
    if config.is_empty() {
        return HashMap::new();
    }

    config
        .split(',')
        .filter_map(|entry| {
            let parts: Vec<&str> = entry.trim().split(':').collect();
            if parts.len() == 3 {
                Some((
                    parts[0].trim().to_string(),
                    (parts[1].trim().to_string(), parts[2].trim().to_string()),
                ))
            } else {
                warn!("Invalid translation group config entry: '{}'", entry);
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_group_config() {
        let config = "group1:en:es,group2:en:fr";
        let groups = parse_group_config(config);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups["group1"], ("en".to_string(), "es".to_string()));
        assert_eq!(groups["group2"], ("en".to_string(), "fr".to_string()));
    }

    #[test]
    fn test_parse_group_config_empty() {
        let groups = parse_group_config("");
        assert!(groups.is_empty());
    }

    #[test]
    fn test_parse_group_config_with_spaces() {
        let config = " group1 : en : es , group2 : en : fr ";
        let groups = parse_group_config(config);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups["group1"], ("en".to_string(), "es".to_string()));
    }

    #[test]
    fn test_parse_group_config_invalid_entries_skipped() {
        let config = "group1:en:es,bad_entry,group2:en:fr";
        let groups = parse_group_config(config);
        assert_eq!(groups.len(), 2);
    }
}
