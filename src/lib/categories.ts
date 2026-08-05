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
  label: string;
  /** System-color-inspired hue used for the storage bar segment and legend dot. */
  color: string;
  icon: LucideIcon;
};

// Colors follow Apple's system palette (systemBlue, systemGreen, etc.) so the
// storage bar reads like the one in System Settings › General › Storage.
const CATEGORY_META: Record<CategoryId, CategoryMeta> = {
  videos: { label: "视频", color: "#0A84FF", icon: Film },
  images: { label: "图片", color: "#34C759", icon: Image },
  documents: { label: "文档", color: "#FF9F0A", icon: FileText },
  audio: { label: "音频", color: "#FF375F", icon: Music },
  code: { label: "代码", color: "#5E5CE6", icon: FileCode },
  archives: { label: "压缩包", color: "#BF5AF2", icon: Archive },
  applications: { label: "应用与安装包", color: "#64D2FF", icon: Package },
  other: { label: "其他", color: "#98989D", icon: File },
};

const FALLBACK: CategoryMeta = CATEGORY_META.other;

export function categoryLabel(category: string): string {
  return (CATEGORY_META[category as CategoryId] ?? FALLBACK).label;
}

export function categoryColor(category: string): string {
  return (CATEGORY_META[category as CategoryId] ?? FALLBACK).color;
}

export function categoryIcon(category: string): LucideIcon {
  return (CATEGORY_META[category as CategoryId] ?? FALLBACK).icon;
}
