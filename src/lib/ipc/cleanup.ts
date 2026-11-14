// Cleanup-hint IPC: reviewable cleanup candidates for the scanned directory.

import { invoke } from "@tauri-apps/api/core";
import type { CleanupSummary, FileEntry } from "../../types/scan";

export function getCleanupSummary(
  scanId: string,
  oldDownloadsDays = 180,
): Promise<CleanupSummary> {
  return invoke("get_cleanup_summary", { scanId, oldDownloadsDays });
}

export function listCleanupFiles(
  scanId: string,
  kind: string,
  limit = 20,
  oldDownloadsDays = 180,
): Promise<FileEntry[]> {
  return invoke("list_cleanup_files", { scanId, kind, limit, oldDownloadsDays });
}
