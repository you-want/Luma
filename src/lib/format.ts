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

export function basename(path: string): string {
  // Trim a trailing separator, then take the last path segment. Handles both
  // POSIX (/) and Windows (\) separators so it works cross-platform.
  const normalized = path.replace(/[/\\]$/, "");
  const segments = normalized.split(/[/\\]/);
  return segments[segments.length - 1] || path;
}
