// File manager IPC: index-derived directory browsing and user-initiated
// write operations (trash/rename/move/copy) plus the persisted undo stack.

import { invoke } from "@tauri-apps/api/core";
import type {
  DirNode,
  DirectoryListing,
  DirFileSort,
  OpResult,
  OperationLogEntry,
  OperationRecord,
  RenameResult,
} from "../../types/fileManager";

export function getDirectoryNodes(
  scanId: string,
  parentPath: string,
): Promise<DirNode[]> {
  return invoke("get_directory_nodes", { scanId, parentPath });
}

export function listDirectoryFiles(
  scanId: string,
  dirPath: string,
  opts: {
    includeHidden?: boolean;
    sort?: DirFileSort;
    limit?: number;
    offset?: number;
  } = {},
): Promise<DirectoryListing> {
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
): Promise<OpResult> {
  return invoke("trash_files", { scanId, paths });
}

export function renameFile(
  scanId: string,
  oldPath: string,
  newName: string,
): Promise<RenameResult> {
  return invoke("rename_file", { scanId, oldPath, newName });
}

export function moveFiles(
  scanId: string,
  paths: string[],
  destDir: string,
): Promise<OpResult> {
  return invoke("move_files", { scanId, paths, destDir });
}

export function copyFiles(
  scanId: string,
  paths: string[],
  destDir: string,
): Promise<OpResult> {
  return invoke("copy_files", { scanId, paths, destDir });
}

export function listUndoableOperations(
  scanId: string,
  limit = 20,
): Promise<OperationRecord[]> {
  return invoke("list_undoable_operations", { scanId, limit });
}

export function listRecentOperations(
  limit = 100,
): Promise<OperationLogEntry[]> {
  return invoke("list_recent_operations", { limit });
}

export function undoFileOperation(
  operationId: string,
  scanId: string,
): Promise<OpResult> {
  return invoke("undo_file_operation", { operationId, scanId });
}
