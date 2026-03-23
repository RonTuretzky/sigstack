use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::detect::{detect_language, lang_to_flag};
use crate::error::TranslationError;
use crate::provider::TranslationProvider;

#[derive(Debug, Clone)]
pub struct TranslatorConfig {
    pub min_message_length: usize,
    pub max_message_length: usize,
    pub confidence_threshold: f64,
    pub rate_limit_per_minute: u32,
}

impl Default for TranslatorConfig {
    fn default() -> Self {
        Self {
            min_message_length: 3,
            max_message_length: 5000,
            confidence_threshold: 0.3,
            rate_limit_per_minute: 60,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TranslationResult {
    pub translated_text: String,
    pub source_lang: String,
    pub target_lang: String,
    pub flag_emoji: String,
}

struct TokenBucket {
    tokens: f64,
    capacity: f64,
    refill_rate: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(per_minute: u32) -> Self {
        let capacity = per_minute as f64;
        Self {
            tokens: capacity,
            capacity,
            refill_rate: capacity / 60.0,
            last_refill: Instant::now(),
        }
    }

    fn try_acquire(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity);
        self.last_refill = now;

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

pub struct Translator {
    provider: Arc<dyn TranslationProvider>,
    config: TranslatorConfig,
    rate_limiter: Mutex<TokenBucket>,
}

impl Translator {
    pub fn new(provider: Arc<dyn TranslationProvider>, config: TranslatorConfig) -> Self {
        let rate_limiter = Mutex::new(TokenBucket::new(config.rate_limit_per_minute));
        Self {
            provider,
            config,
            rate_limiter,
        }
    }

    /// Translate text if it's in one of the two configured languages.
    /// Returns None if the message should be skipped (too short, low confidence, same language, etc.)
    pub async fn translate_if_needed(
        &self,
        text: &str,
        lang_a: &str,
        lang_b: &str,
    ) -> Result<Option<TranslationResult>, TranslationError> {
        // Skip short messages
        if text.len() < self.config.min_message_length {
            debug!("Skipping short message ({} chars)", text.len());
            return Ok(None);
        }

        // Detect language
        let detected = match detect_language(text) {
            Some(d) => d,
            None => {
                debug!("Could not detect language");
                return Ok(None);
            }
        };

        // Check confidence
        if detected.confidence < self.config.confidence_threshold {
            debug!(
                "Low confidence ({:.2}) for detected language '{}'",
                detected.confidence, detected.code
            );
            return Ok(None);
        }

        // Determine target language
        let target = if detected.code == lang_a {
            lang_b
        } else if detected.code == lang_b {
            lang_a
        } else {
            debug!(
                "Detected '{}' not in pair ({}, {})",
                detected.code, lang_a, lang_b
            );
            return Ok(None);
        };

        // Rate limit check
        {
            let mut limiter = self.rate_limiter.lock().await;
            if !limiter.try_acquire() {
                warn!("Translation rate limited");
                return Ok(None);
            }
        }

        // Truncate if needed
        let text_to_translate = if text.len() > self.config.max_message_length {
            // Find the largest valid char boundary not exceeding max_message_length
            let mut safe_end = 0;
            for (idx, ch) in text.char_indices() {
                if idx >= self.config.max_message_length {
                    break;
                }
                safe_end = idx + ch.len_utf8();
            }
            let truncated = &text[..safe_end];
            // Find last word boundary within the truncated text
            let end = truncated.rfind(' ').unwrap_or(truncated.len());
            format!("{}...", &text[..end])
        } else {
            text.to_string()
        };

        // Translate
        let translated = self
            .provider
            .translate(&text_to_translate, &detected.code, target)
            .await?;

        Ok(Some(TranslationResult {
            translated_text: translated,
            source_lang: detected.code,
            target_lang: target.to_string(),
            flag_emoji: lang_to_flag(target).to_string(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = TranslatorConfig::default();
        assert_eq!(config.min_message_length, 3);
        assert_eq!(config.max_message_length, 5000);
    }

    #[test]
    fn test_token_bucket() {
        let mut bucket = TokenBucket::new(60); // 1 per second
        assert!(bucket.try_acquire());
        assert!(bucket.try_acquire());
        // Eventually should run out but with refill it's hard to test precisely
    }
}
