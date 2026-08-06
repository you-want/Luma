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
  /** Luma visualization hue used for the storage bar segment and legend dot. */
  color: string;
  icon: LucideIcon;
};

// The visualization palette stays varied enough to scan while drawing from
// Han's ink, celadon, indigo, bamboo, gold, and ochre color families.
// Labels are not stored here; they live in the i18n resources under
// `category.<id>` and are resolved with `categoryLabelKey` + `t()`.
const CATEGORY_META: Record<CategoryId, CategoryMeta> = {
  videos: { color: "var(--luma-category-videos)", icon: Film },
  images: { color: "var(--luma-category-images)", icon: Image },
  documents: { color: "var(--luma-category-documents)", icon: FileText },
  audio: { color: "var(--luma-category-audio)", icon: Music },
  code: { color: "var(--luma-category-code)", icon: FileCode },
  archives: { color: "var(--luma-category-archives)", icon: Archive },
  applications: { color: "var(--luma-category-applications)", icon: Package },
  other: { color: "var(--luma-category-other)", icon: File },
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

export function categoryTint(category: string): string {
  return `color-mix(in srgb, ${categoryColor(category)} 12%, transparent)`;
}

export function categoryIcon(category: string): LucideIcon {
  return metaFor(category).icon;
}
