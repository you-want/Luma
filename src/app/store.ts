import { useSyncExternalStore } from "react";
import type {
  FileEntry,
  InsightSummary,
  ScanProgress,
  ScanSummary,
} from "../types/scan";

export type AppPhase = "idle" | "running" | "completed" | "cancelled" | "failed";

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
  ready() {
    update({ loading: false });
  },
};

export function useAppStore(): AppState {
  return useSyncExternalStore(appStore.subscribe, appStore.getState);
}
