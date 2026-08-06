import {
  Archive,
  File,
  FileCode,
  FileText,
  Image,
  Music,
  Package,
  Film,
  type LucideIcon,
} from "lucide-react";

// Category identifiers mirror the strings produced by the Rust classifier in
// `scanner.rs`. Keep this list in sync with `classify_extension`.
export type CategoryId =
  | "videos"
  | "images"
  | "documents"
  | "audio"
  | "code"
  | "archives"
  | "applications"
  | "other";

type CategoryMeta = {
  /** System-color-inspired hue used for the storage bar segment and legend dot. */
  color: string;
  icon: LucideIcon;
};

// Colors follow Apple's system palette (systemBlue, systemGreen, etc.) so the
// storage bar reads like the one in System Settings › General › Storage.
// Labels are not stored here; they live in the i18n resources under
// `category.<id>` and are resolved with `categoryLabelKey` + `t()`.
const CATEGORY_META: Record<CategoryId, CategoryMeta> = {
  videos: { color: "#0A84FF", icon: Film },
  images: { color: "#34C759", icon: Image },
  documents: { color: "#FF9F0A", icon: FileText },
  audio: { color: "#FF375F", icon: Music },
  code: { color: "#5E5CE6", icon: FileCode },
  archives: { color: "#BF5AF2", icon: Archive },
  applications: { color: "#64D2FF", icon: Package },
  other: { color: "#98989D", icon: File },
};

const FALLBACK_ID: CategoryId = "other";

function metaFor(category: string): CategoryMeta {
  return CATEGORY_META[category as CategoryId] ?? CATEGORY_META[FALLBACK_ID];
}

// The i18n key for a category label, e.g. "category.videos". Unknown ids fall
// back to "category.other" so the UI never renders a raw category string.
export function categoryLabelKey(category: string): `category.${CategoryId}` {
  const id = (category as CategoryId) in CATEGORY_META ? (category as CategoryId) : FALLBACK_ID;
  return `category.${id}`;
}

export function categoryColor(category: string): string {
  return metaFor(category).color;
}

export function categoryIcon(category: string): LucideIcon {
  return metaFor(category).icon;
}
