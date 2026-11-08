// Agent sessions IPC: local CLI session discovery and trash-only deletion.

import { invoke } from "@tauri-apps/api/core";
import type {
  AgentSessionDeleteItem,
  AgentSessionsReport,
  AgentSessionsTrashOutcome,
} from "../../types/sessions";

export function discoverAgentSessions(): Promise<AgentSessionsReport> {
  return invoke("discover_agent_sessions");
}

export function trashAgentSessions(
  items: AgentSessionDeleteItem[],
): Promise<AgentSessionsTrashOutcome> {
  return invoke("trash_agent_sessions", { items });
}
