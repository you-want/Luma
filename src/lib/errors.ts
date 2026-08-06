import i18n from "../i18n";
import type { Resources } from "../i18n/locales/zh-CN";

type ErrorCode = keyof Resources["errors"];

// Tauri serializes `AppError` as `{ code, message }`. We translate by the
// stable code so the message language follows the UI, and fall back to the
// backend message (already localizable-independent) then a generic string.
function isKnownCode(code: string): code is ErrorCode {
  return i18n.exists(`errors.${code}`);
}

// `fallback` overrides the generic last-resort message when a caller has a more
// specific default for its context (e.g. "failed to load file details").
export function errorMessage(error: unknown, fallback?: string): string {
  if (error && typeof error === "object") {
    const record = error as { code?: unknown; message?: unknown };
    if (typeof record.code === "string" && isKnownCode(record.code)) {
      return i18n.t(`errors.${record.code}` as const);
    }
    if (typeof record.message === "string" && record.message.length > 0) {
      return record.message;
    }
  }
  if (typeof error === "string" && error.length > 0) return error;
  return fallback ?? i18n.t("errors.generic");
}
