import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { enUS } from "./locales/en-US";
import { zhCN, type Resources } from "./locales/zh-CN";
import { setFormatLocale } from "../lib/format";

export const SUPPORTED_LANGUAGES = ["zh-CN", "en-US"] as const;
export type Language = (typeof SUPPORTED_LANGUAGES)[number];

export const DEFAULT_LANGUAGE: Language = "zh-CN";
const STORAGE_KEY = "luma.language";

// Map an arbitrary BCP-47 tag (e.g. from `navigator.language`) onto one of the
// languages we actually ship. Anything Chinese falls back to zh-CN; everything
// else to en-US, so an unshipped locale never shows raw resource keys.
function normalizeLanguage(tag: string | undefined | null): Language {
  if (!tag) return DEFAULT_LANGUAGE;
  const lower = tag.toLowerCase();
  if (lower.startsWith("zh")) return "zh-CN";
  if (lower.startsWith("en")) return "en-US";
  return DEFAULT_LANGUAGE;
}

// Resolution order: an explicit, persisted user choice wins; otherwise detect
// the system language; otherwise the default.
function detectInitialLanguage(): Language {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored && (SUPPORTED_LANGUAGES as readonly string[]).includes(stored)) {
      return stored as Language;
    }
  } catch {
    // localStorage can throw in locked-down webviews; fall through to detection.
  }
  const system =
    typeof navigator !== "undefined"
      ? navigator.languages?.[0] ?? navigator.language
      : undefined;
  return normalizeLanguage(system);
}

const initialLanguage = detectInitialLanguage();

void i18n.use(initReactI18next).init({
  resources: {
    "zh-CN": { translation: zhCN },
    "en-US": { translation: enUS },
  },
  lng: initialLanguage,
  fallbackLng: DEFAULT_LANGUAGE,
  interpolation: { escapeValue: false },
  returnNull: false,
});

setFormatLocale(initialLanguage);

// Keep locale-aware number/date formatting in sync with the active language.
i18n.on("languageChanged", (lng) => {
  setFormatLocale(normalizeLanguage(lng));
});

export function changeLanguage(language: Language): void {
  try {
    localStorage.setItem(STORAGE_KEY, language);
  } catch {
    // Persistence is best-effort; the in-memory switch still applies.
  }
  void i18n.changeLanguage(language);
}

export function currentLanguage(): Language {
  return normalizeLanguage(i18n.language);
}

// Typed keys: `t("scanControls.scan")` is checked, unknown keys error.
declare module "i18next" {
  interface CustomTypeOptions {
    defaultNS: "translation";
    resources: { translation: Resources };
  }
}

export default i18n;
