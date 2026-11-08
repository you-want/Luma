// Smart organizer IPC: read-only dry-run planning and confirmed execution.

import { invoke } from "@tauri-apps/api/core";
import type {
  OrganizeMoveInput,
  OrganizeOptions,
  OrganizePlan,
  OrganizeResult,
  OrganizeRule,
} from "../../types/fileManager";

export function planOrganize(
  scanId: string,
  sourceDir: string,
  destDir: string,
  rule: OrganizeRule,
  options: OrganizeOptions,
): Promise<OrganizePlan> {
  return invoke("plan_organize", { scanId, sourceDir, destDir, rule, options });
}

export function executeOrganizePlan(
  scanId: string,
  moves: OrganizeMoveInput[],
): Promise<OrganizeResult> {
  return invoke("execute_organize_plan", { scanId, moves });
}
