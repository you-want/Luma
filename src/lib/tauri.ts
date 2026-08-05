import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import type {
  DuplicateGroup,
  FileEntry,
  InsightKind,
  InsightSummary,
  ProjectCandidate,
  ScanSummary,
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

export function revealPath(path: string): Promise<void> {
  return revealItemInDir(path);
}
