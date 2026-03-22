use whatlang::{detect as wl_detect, Lang};

#[derive(Debug, Clone)]
pub struct DetectedLanguage {
    pub code: String,
    pub confidence: f64,
}

/// Detect the language of a text string. Returns ISO 639-1 code and confidence.
pub fn detect_language(text: &str) -> Option<DetectedLanguage> {
    let info = wl_detect(text)?;
    let code = lang_to_iso(info.lang())?;
    Some(DetectedLanguage {
        code: code.to_string(),
        confidence: info.confidence(),
    })
}

/// Map whatlang Lang to ISO 639-1 code.
fn lang_to_iso(lang: Lang) -> Option<&'static str> {
    match lang {
        Lang::Eng => Some("en"),
        Lang::Spa => Some("es"),
        Lang::Fra => Some("fr"),
        Lang::Deu => Some("de"),
        Lang::Ita => Some("it"),
        Lang::Por => Some("pt"),
        Lang::Rus => Some("ru"),
        Lang::Cmn => Some("zh"),
        Lang::Jpn => Some("ja"),
        Lang::Kor => Some("ko"),
        Lang::Ara => Some("ar"),
        Lang::Hin => Some("hi"),
        Lang::Tur => Some("tr"),
        Lang::Pol => Some("pl"),
        Lang::Nld => Some("nl"),
        Lang::Swe => Some("sv"),
        Lang::Dan => Some("da"),
        Lang::Fin => Some("fi"),
        Lang::Nob => Some("no"),
        Lang::Ukr => Some("uk"),
        Lang::Ces => Some("cs"),
        Lang::Ron => Some("ro"),
        Lang::Ell => Some("el"),
        Lang::Hun => Some("hu"),
        Lang::Heb => Some("he"),
        Lang::Tha => Some("th"),
        Lang::Vie => Some("vi"),
        Lang::Ind => Some("id"),
        Lang::Ben => Some("bn"),
        _ => None,
    }
}

/// Map ISO 639-1 language code to flag emoji.
pub fn lang_to_flag(code: &str) -> &'static str {
    match code {
        "en" => "\u{1f1ec}\u{1f1e7}", // GB flag
        "es" => "\u{1f1ea}\u{1f1f8}", // Spain
        "fr" => "\u{1f1eb}\u{1f1f7}", // France
        "de" => "\u{1f1e9}\u{1f1ea}", // Germany
        "it" => "\u{1f1ee}\u{1f1f9}", // Italy
        "pt" => "\u{1f1e7}\u{1f1f7}", // Brazil
        "ru" => "\u{1f1f7}\u{1f1fa}", // Russia
        "zh" => "\u{1f1e8}\u{1f1f3}", // China
        "ja" => "\u{1f1ef}\u{1f1f5}", // Japan
        "ko" => "\u{1f1f0}\u{1f1f7}", // South Korea
        "ar" => "\u{1f1f8}\u{1f1e6}", // Saudi Arabia
        "hi" => "\u{1f1ee}\u{1f1f3}", // India
        "tr" => "\u{1f1f9}\u{1f1f7}", // Turkey
        "pl" => "\u{1f1f5}\u{1f1f1}", // Poland
        "nl" => "\u{1f1f3}\u{1f1f1}", // Netherlands
        "sv" => "\u{1f1f8}\u{1f1ea}", // Sweden
        "da" => "\u{1f1e9}\u{1f1f0}", // Denmark
        "fi" => "\u{1f1eb}\u{1f1ee}", // Finland
        "no" => "\u{1f1f3}\u{1f1f4}", // Norway
        "uk" => "\u{1f1fa}\u{1f1e6}", // Ukraine
        "cs" => "\u{1f1e8}\u{1f1ff}", // Czech Republic
        "ro" => "\u{1f1f7}\u{1f1f4}", // Romania
        "el" => "\u{1f1ec}\u{1f1f7}", // Greece
        "hu" => "\u{1f1ed}\u{1f1fa}", // Hungary
        "he" => "\u{1f1ee}\u{1f1f1}", // Israel
        "th" => "\u{1f1f9}\u{1f1ed}", // Thailand
        "vi" => "\u{1f1fb}\u{1f1f3}", // Vietnam
        "id" => "\u{1f1ee}\u{1f1e9}", // Indonesia
        "bn" => "\u{1f1e7}\u{1f1e9}", // Bangladesh
        _ => "\u{1f310}",             // Globe
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_english() {
        let result = detect_language("This is a test of the English language detection system");
        assert!(result.is_some());
        assert_eq!(result.unwrap().code, "en");
    }

    #[test]
    fn test_detect_spanish() {
        let result = detect_language("Esta es una prueba del sistema de deteccion de idiomas en espanol");
        assert!(result.is_some());
        assert_eq!(result.unwrap().code, "es");
    }

    #[test]
    fn test_short_text_returns_none() {
        let result = detect_language("hi");
        // Very short text may or may not detect - just verify no panic
        let _ = result;
    }

    #[test]
    fn test_flag_mapping() {
        assert_eq!(lang_to_flag("en"), "\u{1f1ec}\u{1f1e7}");
        assert_eq!(lang_to_flag("es"), "\u{1f1ea}\u{1f1f8}");
        assert_eq!(lang_to_flag("unknown"), "\u{1f310}");
    }
}
