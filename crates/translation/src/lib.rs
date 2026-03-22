pub mod detect;
pub mod error;
pub mod provider;
pub mod translator;

pub use detect::{detect_language, lang_to_flag, DetectedLanguage};
pub use error::TranslationError;
pub use provider::{TranslationProvider, libretranslate::LibreTranslateProvider};
pub use translator::{TranslationResult, Translator, TranslatorConfig};
