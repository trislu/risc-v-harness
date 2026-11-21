//! One-line descriptions, compiled from `content/overlay/<locale>/<kind>.toml`.
//!
//! Lookup falls back to English when a locale does not have a key. That is the
//! whole point of the overlay: a translation can land one line at a time.

#![forbid(unsafe_code)]

mod generated;

/// Which table to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Pseudo,
    Directive,
    Register,
}

impl Kind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pseudo => "pseudos",
            Self::Directive => "directives",
            Self::Register => "registers",
        }
    }
}

/// Map a locale tag onto the directory name that carries its overlay.
///
/// `zh`, `zh-CN`, `zh-Hans` all land on `zh-CN`. Anything else falls back to
/// `en` at lookup time, so an unknown locale degrades instead of failing.
#[must_use]
pub fn normalize_locale(locale: &str) -> &'static str {
    if locale.to_ascii_lowercase().starts_with("zh") {
        "zh-CN"
    } else {
        "en"
    }
}

/// Text for `name` in `kind`, in `locale`, falling back to English.
#[must_use]
pub fn text(locale: &str, kind: Kind, name: &str) -> Option<&'static str> {
    lookup(normalize_locale(locale), kind.as_str(), name)
        .or_else(|| lookup("en", kind.as_str(), name))
}

/// Role of a register, by number.
#[must_use]
pub fn register_role(locale: &str, index: u8) -> Option<&'static str> {
    let key = index.to_string();
    text(locale, Kind::Register, &key)
}

fn lookup(locale: &str, kind: &str, name: &str) -> Option<&'static str> {
    generated::table(locale, kind)?
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| *value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_is_complete() {
        assert!(text("en", Kind::Pseudo, "li").is_some());
        assert!(text("en", Kind::Directive, ".text").is_some());
        assert!(register_role("en", 10).is_some());
    }

    #[test]
    fn chinese_covers_the_translated_keys() {
        let translated = text("zh-CN", Kind::Pseudo, "li").expect("translated");
        assert!(translated.contains("加载"), "{translated}");
    }

    #[test]
    fn untranslated_keys_fall_back_to_english() {
        let english = text("en", Kind::Pseudo, "neg").expect("english");
        assert_eq!(text("zh-CN", Kind::Pseudo, "neg"), Some(english));
        // Directives have no translation yet at all.
        assert_eq!(
            text("zh-CN", Kind::Directive, ".text"),
            text("en", Kind::Directive, ".text")
        );
        assert_eq!(register_role("zh-CN", 2), register_role("en", 2));
    }

    #[test]
    fn locale_tags_normalise() {
        assert_eq!(normalize_locale("zh"), "zh-CN");
        assert_eq!(normalize_locale("zh-Hans"), "zh-CN");
        assert_eq!(normalize_locale("fr"), "en");
    }

    #[test]
    fn unknown_names_are_none() {
        assert_eq!(text("en", Kind::Pseudo, "frobnicate"), None);
    }
}
