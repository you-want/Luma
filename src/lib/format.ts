const BYTE_UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

// The active locale for number/date formatting. Updated by the i18n layer on
// language change (see `src/i18n/index.ts`) so formatting follows the UI
// language without threading a locale argument through every call site.
let currentLocale = "zh-CN";

export function setFormatLocale(locale: string): void {
  currentLocale = locale;
}

export function formatBytes(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return "0 B";
  const unitIndex = Math.min(
    Math.floor(Math.log(value) / Math.log(1024)),
    BYTE_UNITS.length - 1,
  );
  const amount = value / 1024 ** unitIndex;
  const digits = unitIndex === 0 || amount >= 100 ? 0 : amount >= 10 ? 1 : 2;
  return `${amount.toFixed(digits)} ${BYTE_UNITS[unitIndex]}`;
}

export function formatNumber(value: number): string {
  return new Intl.NumberFormat(currentLocale).format(value);
}

export function formatDate(timestamp?: number): string {
  if (!timestamp) return "--";
  return new Intl.DateTimeFormat(currentLocale, {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(timestamp * 1000));
}

/** "1 分钟前" / "1 minute ago" style, following the UI language. */
export function formatRelativeTime(timestamp?: number | null): string {
  if (!timestamp) return "--";
  const seconds = Math.round(Date.now() / 1000 - timestamp);
  const formatter = new Intl.RelativeTimeFormat(currentLocale, { numeric: "always" });
  if (seconds < 60) return formatter.format(-Math.max(seconds, 0), "second");
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return formatter.format(-minutes, "minute");
  const hours = Math.round(minutes / 60);
  if (hours < 24) return formatter.format(-hours, "hour");
  const days = Math.round(hours / 24);
  return formatter.format(-days, "day");
}

export function basename(path: string): string {
  // Trim a trailing separator, then take the last path segment. Handles both
  // POSIX (/) and Windows (\) separators so it works cross-platform.
  const normalized = path.replace(/[/\\]$/, "");
  const segments = normalized.split(/[/\\]/);
  return segments[segments.length - 1] || path;
}
