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
#[derive(Clone)]
pub struct TrayStrings {
    pub language: Language,
    pub show: &'static str,
    pub quit: &'static str,
    pub tooltip: &'static str,
    /// Toggle label for tray performance monitoring (CLN-403).
    pub monitor_toggle: &'static str,
    /// Placeholder text for the performance line while monitoring is off.
    pub performance_off: &'static str,
    /// Placeholder shown briefly right after enabling, before the first
    /// sample lands.
    pub sampling: &'static str,
}

impl TrayStrings {
    /// Menu line showing the latest CPU/memory sample, e.g.
    /// "CPU 12% · 内存 62%".
    pub fn performance_text(&self, cpu: f32, memory_percent: f32) -> String {
        match self.language {
            Language::ZhCn => format!("CPU {:.0}% · 内存 {:.0}%", cpu, memory_percent),
            Language::EnUs => format!("CPU {:.0}% · MEM {:.0}%", cpu, memory_percent),
        }
    }

    /// Tooltip while monitoring is on.
    pub fn performance_tooltip(&self, cpu: f32, memory_percent: f32) -> String {
        match self.language {
            Language::ZhCn => format!("Luma · CPU {:.0}% · 内存 {:.0}%", cpu, memory_percent),
            Language::EnUs => format!("Luma · CPU {:.0}% · MEM {:.0}%", cpu, memory_percent),
        }
    }
}

pub fn tray_strings(language: Language) -> TrayStrings {
    match language {
        Language::ZhCn => TrayStrings {
            language,
            show: "显示 Luma",
            quit: "退出 Luma",
            tooltip: "Luma · 本地空间观察",
            monitor_toggle: "在托盘显示系统性能",
            performance_off: "性能监测已关闭",
            sampling: "正在采样…",
        },
        Language::EnUs => TrayStrings {
            language,
            show: "Show Luma",
            quit: "Quit Luma",
            tooltip: "Luma · Local Space Insight",
            monitor_toggle: "Show system performance in tray",
            performance_off: "Performance monitoring off",
            sampling: "Sampling…",
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
