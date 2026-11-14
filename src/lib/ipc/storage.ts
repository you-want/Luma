// Local-storage IPC: Luma's own index usage, compaction and clearing.

import { invoke } from "@tauri-apps/api/core";

export type IndexStorageUsage = {
  databaseBytes: number;
  journalBytes: number;
  reclaimableBytes: number;
  scanCount: number;
  indexedFiles: number;
  undoRecords: number;
  scanBudgetBytes: number;
  maintenanceError?: string;
};

export function getIndexStorage(): Promise<IndexStorageUsage> {
  return invoke("get_index_storage");
}

export function compactIndexStorage(): Promise<IndexStorageUsage> {
  return invoke("compact_index_storage");
}

export function clearScanIndex(): Promise<IndexStorageUsage> {
  return invoke("clear_scan_index");
}
