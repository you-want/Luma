// System-feature IPC: app slimming, maintenance, system status, tray monitor
// and the machine-wide slimming family (project purge, orphan app data).

import { invoke } from "@tauri-apps/api/core";
import type {
  AppDataReport,
  MaintenanceOutcome,
  MaintenanceReport,
  PurgeReport,
  SlimOutcome,
  SlimReport,
  SlimRequestItem,
  SystemStatus,
} from "../../types/scan";
import type { CleanRequestItem, CleanOutcome } from "../../types/scan";

// ── App slimming (应用瘦身) ────────────────────────────────────────────────────

export function discoverSlimCandidates(): Promise<SlimReport> {
  return invoke("discover_slim_candidates");
}

export function executeSlim(items: SlimRequestItem[]): Promise<SlimOutcome> {
  return invoke("execute_slim", { items });
}

// ── System maintenance & status (系统维护 / 系统状态) ──────────────────────────

export function getMaintenanceTasks(): Promise<MaintenanceReport> {
  return invoke("get_maintenance_tasks");
}

export function runMaintenanceTask(task: string): Promise<MaintenanceOutcome> {
  return invoke("run_maintenance_task", { task });
}

export function getSystemStatus(): Promise<SystemStatus> {
  return invoke("get_system_status");
}

export function getTrayMonitorEnabled(): Promise<boolean> {
  return invoke("get_tray_monitor_enabled");
}

export function setTrayMonitorEnabled(enabled: boolean): Promise<void> {
  return invoke("set_tray_monitor_enabled", { enabled });
}

// ── Project slimming (项目瘦身) ────────────────────────────────────────────────

export function discoverPurgeProjects(): Promise<PurgeReport> {
  return invoke("discover_purge_projects");
}

export function executePurge(items: CleanRequestItem[]): Promise<CleanOutcome> {
  return invoke("execute_purge", { items });
}

// ── Orphaned app data (应用残留清理) ───────────────────────────────────────────

export function discoverOrphanAppData(): Promise<AppDataReport> {
  return invoke("discover_orphan_app_data");
}

export function executeOrphanClean(items: CleanRequestItem[]): Promise<CleanOutcome> {
  return invoke("execute_orphan_clean", { items });
}
