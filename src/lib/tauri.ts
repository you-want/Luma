import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  CleanupSummary,
  DuplicateGroup,
  FileEntry,
  InsightKind,
  InsightSummary,
  ProjectCandidate,
  ScanComparison,
  ScanSummary,
  SearchRequest,
  SearchResponse,
  StartScanRequest,
} from "../types/scan";

export async function chooseDirectory(): Promise<string | null> {
  const selected = await open({ directory: true, multiple: false });
  return typeof selected === "string" ? selected : null;
}

export function startScan(request: StartScanRequest): Promise<string> {
  return invoke("start_scan", { request });
}

export function cancelScan(scanId: string): Promise<void> {
  return invoke("cancel_scan", { scanId });
}

export function getLatestScan(): Promise<ScanSummary | null> {
  return invoke("get_latest_scan");
}

export function getScanSummary(scanId: string): Promise<ScanSummary> {
  return invoke("get_scan_summary", { scanId });
}

export function listLargeFiles(
  scanId: string,
  limit = 20,
  offset = 0,
): Promise<FileEntry[]> {
  return invoke("list_large_files", { scanId, limit, offset });
}

export function listInsights(
  scanId: string,
  largeFileThreshold: number,
  staleDays: number,
): Promise<InsightSummary[]> {
  return invoke("list_insights", { scanId, largeFileThreshold, staleDays });
}

export function listInsightFiles(
  scanId: string,
  kind: InsightKind,
  largeFileThreshold: number,
  staleDays: number,
  limit = 10,
): Promise<FileEntry[]> {
  return invoke("list_insight_files", {
    scanId,
    kind,
    largeFileThreshold,
    staleDays,
    limit,
  });
}

export function findDuplicates(
  scanId: string,
  minSize?: number,
): Promise<DuplicateGroup[]> {
  return invoke("find_duplicates", { scanId, minSize });
}

export function listProjects(scanId: string): Promise<ProjectCandidate[]> {
  return invoke("list_projects", { scanId });
}

export function listScanHistory(scanId: string): Promise<ScanSummary[]> {
  return invoke("list_scan_history", { scanId });
}

export function compareScans(
  baseScanId: string,
  targetScanId: string,
): Promise<ScanComparison> {
  return invoke("compare_scans", { baseScanId, targetScanId });
}

export function searchFiles(request: SearchRequest): Promise<SearchResponse> {
  return invoke("search_files", { request });
}

export function revealPath(path: string): Promise<void> {
  // Routed through a Rust command (not the opener plugin directly) so the
  // canonical `/`-separated stored path is converted back to native separators
  // before the OS file manager selects the item. See `commands::reveal_path`.
  return invoke("reveal_path", { path });
}

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
