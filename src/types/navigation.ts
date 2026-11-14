// Top-level navigation model shared by the sidebar, the command palette and
// the app shell. Group order drives both the sidebar rendering and the
// keyboard navigation cycle.

export type AppView =
  | "overview"
  | "analysis"
  | "files"
  | "organize"
  | "cleaner"
  | "sessions"
  | "history"
  | "settings";

export type NavGroupKey = "workspace" | "machine" | "system";

export const NAV_GROUPS: readonly { key: NavGroupKey; views: readonly AppView[] }[] = [
  { key: "workspace", views: ["overview", "analysis", "files", "organize"] },
  { key: "machine", views: ["cleaner", "sessions"] },
  { key: "system", views: ["history", "settings"] },
];

export const ALL_VIEWS: readonly AppView[] = NAV_GROUPS.flatMap((group) => group.views);

/** Views that operate on the whole machine or on Luma itself — no scan gate. */
export const SCAN_FREE_VIEWS: readonly AppView[] = [
  "overview",
  "cleaner",
  "sessions",
  "history",
  "settings",
];

export function isScanFreeView(view: AppView): boolean {
  return SCAN_FREE_VIEWS.includes(view);
}

/** Which contextual top bar a view renders. */
export type TopbarMode = "workspace" | "machine" | "plain";

export function topbarModeFor(view: AppView): TopbarMode {
  if (view === "overview" || view === "analysis" || view === "files" || view === "organize") {
    return "workspace";
  }
  if (view === "cleaner" || view === "sessions") return "machine";
  return "plain";
}
