use thiserror::Error;

#[derive(Error, Debug)]
pub enum TranslationError {
    #[error("Language detection failed")]
    DetectionFailed,

    #[error("Translation failed: {0}")]
    TranslationFailed(String),

    #[error("Rate limited")]
    RateLimited,

    #[error("Unsupported language: {0}")]
    UnsupportedLanguage(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("API error: {0}")]
    ApiError(String),
}
