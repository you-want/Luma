// Disk cleaner IPC: fixed-location cleanup center, whitelist and history.

import { invoke } from "@tauri-apps/api/core";
import type {
  CleanOutcome,
  CleanRequestItem,
  CleanerFile,
  CleanerOperation,
  CleanerOverview,
  CleanerWhitelistEntry,
} from "../../types/scan";

export function getCleanerOverview(): Promise<CleanerOverview> {
  return invoke("get_cleaner_overview");
}

export function listCleanerFiles(kind: string, limit = 20): Promise<CleanerFile[]> {
  return invoke("list_cleaner_files", { kind, limit });
}

export function executeCleaner(
  kind: string,
  items: CleanRequestItem[],
): Promise<CleanOutcome> {
  return invoke("execute_cleaner", { kind, items });
}

export function getCleanerWhitelist(): Promise<CleanerWhitelistEntry[]> {
  return invoke("get_cleaner_whitelist");
}

export function addCleanerWhitelist(path: string): Promise<CleanerWhitelistEntry[]> {
  return invoke("add_cleaner_whitelist", { path });
}

export function removeCleanerWhitelist(path: string): Promise<CleanerWhitelistEntry[]> {
  return invoke("remove_cleaner_whitelist", { path });
}

export function listCleanerOperations(limit = 20): Promise<CleanerOperation[]> {
  return invoke("list_cleaner_operations", { limit });
}
