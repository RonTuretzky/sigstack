pub mod libretranslate;

use async_trait::async_trait;
use crate::error::TranslationError;

#[async_trait]
pub trait TranslationProvider: Send + Sync {
    async fn translate(&self, text: &str, from: &str, to: &str) -> Result<String, TranslationError>;
    fn name(&self) -> &str;
}
