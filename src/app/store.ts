import { useSyncExternalStore } from "react";
import type {
  DuplicateGroup,
  FileEntry,
  InsightSummary,
  ScanProgress,
  ScanSummary,
} from "../types/scan";

export type AppPhase = "idle" | "running" | "completed" | "cancelled" | "failed";

/** Shared duplicate-finding state so the overview card and the analysis
 * section show the same result without running the content hashing twice. */
export type DuplicatesState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "ready"; groups: DuplicateGroup[] };

type AppState = {
  phase: AppPhase;
  selectedPath: string;
  activeScanId?: string;
  progress?: ScanProgress;
  summary?: ScanSummary;
  largeFiles: FileEntry[];
  insights: InsightSummary[];
  includeHidden: boolean;
  largeFileThreshold: number;
  staleDays: number;
  error?: string;
  loading: boolean;
  /** Bumped after any on-disk mutation so other views can reload their data. */
  dataRevision: number;
  /** Prefill for the organize page: set when navigating with a source dir. */
  organizeSeed: { sourceDir: string; rev: number } | null;
  /** Last directory the file manager was browsing, so a rescan (which
   * unmounts the panel) can restore the position instead of jumping to root. */
  fileManagerPath: string | null;
  duplicates: DuplicatesState;
};

let state: AppState = {
  phase: "idle",
  selectedPath: "",
  largeFiles: [],
  insights: [],
  includeHidden: false,
  largeFileThreshold: 1024 ** 3,
  staleDays: 180,
  loading: true,
  dataRevision: 0,
  organizeSeed: null,
  fileManagerPath: null,
  duplicates: { status: "idle" },
};
const listeners = new Set<() => void>();

function update(patch: Partial<AppState>) {
  state = { ...state, ...patch };
  listeners.forEach((listener) => listener());
}

export const appStore = {
  getState: () => state,
  subscribe(listener: () => void) {
    listeners.add(listener);
    return () => listeners.delete(listener);
  },
  setSelectedPath(selectedPath: string) {
    update({ selectedPath, error: undefined });
  },
  setIncludeHidden(includeHidden: boolean) {
    update({ includeHidden });
  },
  start(activeScanId: string) {
    update({
      phase: "running",
      activeScanId,
      progress: undefined,
      summary: undefined,
      largeFiles: [],
      insights: [],
      error: undefined,
      loading: false,
      duplicates: { status: "idle" },
    });
  },
  setProgress(progress: ScanProgress) {
    if (progress.scanId === state.activeScanId) update({ progress });
  },
  complete(
    summary: ScanSummary,
    largeFiles: FileEntry[],
    insights: InsightSummary[],
  ) {
    update({
      phase: "completed",
      activeScanId: undefined,
      selectedPath: summary.rootPath,
      summary,
      largeFiles,
      insights,
      error: undefined,
      loading: false,
    });
  },
  setDiscoverySettings(largeFileThreshold: number, staleDays: number) {
    update({ largeFileThreshold, staleDays });
  },
  setInsights(insights: InsightSummary[]) {
    update({ insights });
  },
  setDuplicates(duplicates: DuplicatesState) {
    update({ duplicates });
  },
  bumpDataRevision() {
    update({ dataRevision: state.dataRevision + 1 });
  },
  seedOrganize(sourceDir: string) {
    update({ organizeSeed: { sourceDir, rev: state.dataRevision + 1 } });
  },
  setFileManagerPath(path: string) {
    update({ fileManagerPath: path });
  },
  cancel() {
    update({
      phase: "cancelled",
      activeScanId: undefined,
      progress: undefined,
      error: undefined,
      loading: false,
    });
  },
  fail(error: string) {
    update({
      phase: "failed",
      activeScanId: undefined,
      progress: undefined,
      error,
      loading: false,
    });
  },
  clearIndex() {
    update({ phase: "idle", activeScanId: undefined, progress: undefined, summary: undefined,
      largeFiles: [], insights: [], error: undefined, loading: false,
      duplicates: { status: "idle" } });
  },
  ready() {
    update({ loading: false });
  },
};

export function useAppStore(): AppState {
  return useSyncExternalStore(appStore.subscribe, appStore.getState);
}
