import { useTranslation } from "react-i18next";
import { Languages } from "lucide-react";
import {
  changeLanguage,
  currentLanguage,
  SUPPORTED_LANGUAGES,
  type Language,
} from "../i18n";

// A compact language selector for the header. The current language is read from
// i18n (not local state) so it stays correct after a persisted choice is
// restored on startup, and `changeLanguage` both persists and applies it.
export function LanguageSwitcher() {
  const { t, i18n } = useTranslation();
  // Reference i18n.language so the component re-renders on language change.
  void i18n.language;
  const active = currentLanguage();

  return (
    <label className="language-switcher" title={t("lang.label")}>
      <Languages size={15} aria-hidden="true" />
      <span className="visually-hidden">{t("lang.label")}</span>
      <select
        value={active}
        aria-label={t("lang.label")}
        onChange={(event) => changeLanguage(event.target.value as Language)}
      >
        {SUPPORTED_LANGUAGES.map((language) => (
          <option key={language} value={language}>
            {t(`lang.${language}` as const)}
          </option>
        ))}
      </select>
    </label>
  );
}
