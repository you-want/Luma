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

// ── File Manager ──────────────────────────────────────────────────────────────

export function getDirectoryNodes(
  scanId: string,
  parentPath: string,
): Promise<import("../types/fileManager").DirNode[]> {
  return invoke("get_directory_nodes", { scanId, parentPath });
}

export function listDirectoryFiles(
  scanId: string,
  dirPath: string,
  opts: {
    includeHidden?: boolean;
    sort?: import("../types/fileManager").DirFileSort;
    limit?: number;
    offset?: number;
  } = {},
): Promise<import("../types/fileManager").DirectoryListing> {
  return invoke("list_directory_files", {
    scanId,
    dirPath,
    includeHidden: opts.includeHidden ?? false,
    sort: opts.sort ?? "nameAsc",
    limit: opts.limit ?? 100,
    offset: opts.offset ?? 0,
  });
}

export function openPath(path: string): Promise<void> {
  return invoke("open_path", { path });
}

export function trashFiles(
  scanId: string,
  paths: string[],
): Promise<import("../types/fileManager").OpResult> {
  return invoke("trash_files", { scanId, paths });
}

export function renameFile(
  scanId: string,
  oldPath: string,
  newName: string,
): Promise<import("../types/fileManager").RenameResult> {
  return invoke("rename_file", { scanId, oldPath, newName });
}

export function moveFiles(
  scanId: string,
  paths: string[],
  destDir: string,
): Promise<import("../types/fileManager").OpResult> {
  return invoke("move_files", { scanId, paths, destDir });
}

export function copyFiles(
  scanId: string,
  paths: string[],
  destDir: string,
): Promise<import("../types/fileManager").OpResult> {
  return invoke("copy_files", { scanId, paths, destDir });
}

export function listUndoableOperations(
  scanId: string,
  limit = 20,
): Promise<import("../types/fileManager").OperationRecord[]> {
  return invoke("list_undoable_operations", { scanId, limit });
}

export function undoFileOperation(
  operationId: string,
  scanId: string,
): Promise<import("../types/fileManager").OpResult> {
  return invoke("undo_file_operation", { operationId, scanId });
}

// ── Smart organizer ────────────────────────────────────────────────────────────

export function planOrganize(
  scanId: string,
  sourceDir: string,
  destDir: string,
  rule: import("../types/fileManager").OrganizeRule,
): Promise<import("../types/fileManager").OrganizePlan> {
  return invoke("plan_organize", { scanId, sourceDir, destDir, rule });
}

export function executeOrganizePlan(
  scanId: string,
  moves: import("../types/fileManager").OrganizeMoveInput[],
): Promise<import("../types/fileManager").OrganizeResult> {
  return invoke("execute_organize_plan", { scanId, moves });
}

// ── Cleanup ────────────────────────────────────────────────────────────────────

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
