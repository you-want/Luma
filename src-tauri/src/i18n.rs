//! Minimal localization for backend-owned, non-error strings (currently the
//! tray menu and tooltip). User-facing scan/result text is translated in the
//! frontend; error surfaces carry a stable `code` the frontend translates.
//!
//! The tray is built once at startup, so its language follows the detected
//! system locale. A live switch from the in-app language selector would need
//! the frontend to re-emit the tray labels; that is deliberately out of scope
//! for the first i18n pass and tracked as a follow-up.

/// Supported UI languages. Mirrors the frontend `SUPPORTED_LANGUAGES`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    ZhCn,
    EnUs,
}

impl Language {
    /// Map an arbitrary BCP-47 tag (e.g. "en-US", "zh-Hans-CN") onto a shipped
    /// language. Anything Chinese is zh-CN; everything else falls back to the
    /// default so an unshipped locale never yields an empty label.
    pub fn from_tag(tag: &str) -> Self {
        let lower = tag.to_ascii_lowercase();
        if lower.starts_with("zh") {
            Language::ZhCn
        } else {
            Language::EnUs
        }
    }

    /// Detect the system language, defaulting to zh-CN when detection fails so
    /// the historical Chinese-first behavior is preserved.
    pub fn detect() -> Self {
        match sys_locale::get_locale() {
            Some(tag) => Language::from_tag(&tag),
            None => Language::ZhCn,
        }
    }
}

/// Tray/system strings for a given language.
pub struct TrayStrings {
    pub show: &'static str,
    pub quit: &'static str,
    pub tooltip: &'static str,
}

pub fn tray_strings(language: Language) -> TrayStrings {
    match language {
        Language::ZhCn => TrayStrings {
            show: "显示 Luma",
            quit: "退出 Luma",
            tooltip: "Luma · 本地空间观察",
        },
        Language::EnUs => TrayStrings {
            show: "Show Luma",
            quit: "Quit Luma",
            tooltip: "Luma · Local Space Insight",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_language_tags() {
        assert_eq!(Language::from_tag("zh-CN"), Language::ZhCn);
        assert_eq!(Language::from_tag("zh-Hans-CN"), Language::ZhCn);
        assert_eq!(Language::from_tag("en-US"), Language::EnUs);
        assert_eq!(Language::from_tag("fr-FR"), Language::EnUs);
        assert_eq!(Language::from_tag(""), Language::EnUs);
    }

    #[test]
    fn tray_strings_are_non_empty_for_all_languages() {
        for language in [Language::ZhCn, Language::EnUs] {
            let strings = tray_strings(language);
            assert!(!strings.show.is_empty());
            assert!(!strings.quit.is_empty());
            assert!(!strings.tooltip.is_empty());
        }
    }
}
