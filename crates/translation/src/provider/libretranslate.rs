use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::debug;

use crate::error::TranslationError;
use crate::provider::TranslationProvider;

pub struct LibreTranslateProvider {
    base_url: String,
    api_key: Option<String>,
    client: Client,
}

#[derive(Serialize)]
struct LibreTranslateRequest<'a> {
    q: &'a str,
    source: &'a str,
    target: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_key: Option<&'a str>,
}

#[derive(Deserialize)]
struct LibreTranslateResponse {
    #[serde(rename = "translatedText")]
    translated_text: String,
}

impl LibreTranslateProvider {
    pub fn new(base_url: String, api_key: Option<String>) -> Self {
        Self {
            base_url,
            api_key,
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .connect_timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }
}

#[async_trait]
impl TranslationProvider for LibreTranslateProvider {
    async fn translate(&self, text: &str, from: &str, to: &str) -> Result<String, TranslationError> {
        debug!("LibreTranslate {} -> {} ({} chars)", from, to, text.len());

        let request = LibreTranslateRequest {
            q: text,
            source: from,
            target: to,
            api_key: self.api_key.as_deref(),
        };

        let response = self
            .client
            .post(format!("{}/translate", self.base_url))
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(TranslationError::RateLimited);
        }
        if status == reqwest::StatusCode::FORBIDDEN {
            return Err(TranslationError::ApiError("LibreTranslate: forbidden (check API key)".into()));
        }
        if !status.is_success() {
            let msg = response.text().await.unwrap_or_default();
            return Err(TranslationError::ApiError(format!("LibreTranslate {} - {}", status, msg)));
        }

        let result: LibreTranslateResponse = response.json().await?;
        Ok(result.translated_text)
    }

    fn name(&self) -> &str {
        "libretranslate"
    }
}
