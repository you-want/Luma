// Agent sessions (Agent 会话) — mirrored from src-tauri/src/sessions.rs.
// Split from scan.ts so neither side grows unmanageable (same rationale as
// fileManager.ts); trash outcome types are reused from scan.ts.

import type { CleanOutcome } from "./scan";

/** Provider ids supported by the backend registry (one entry per agent CLI). */
export type AgentId = "claude" | "codex" | "gemini";

export type AgentSession = {
  /** Session id from the transcript (fallback: file stem). */
  id: string;
  agent: AgentId;
  /** Working directory the session ran in ("" when unknown). */
  projectDir: string;
  /** Basename of projectDir ("" when unknown). */
  projectName: string;
  /** Display title: custom title → generated title → first user message → project name. */
  title: string;
  /** Preview of the most recent message (≤ 160 chars). */
  summary: string;
  /** Preview of the first real user message (≤ 160 chars). */
  firstMessage: string;
  /** Epoch seconds, from the transcript when available, else file mtime. */
  createdAt: number | null;
  /** Epoch seconds of the last transcript record. */
  lastActiveAt: number | null;
  sizeBytes: number;
  /** Canonical `/`-separated transcript path; the delete-request key. */
  sourcePath: string;
  /** Shell command that resumes this session (run inside projectDir). */
  resumeCommand: string;
};

export type AgentInfo = {
  id: AgentId;
  sessionCount: number;
};

export type AgentSessionsReport = {
  agents: AgentInfo[];
  /** Newest first by last activity. */
  sessions: AgentSession[];
};

export type AgentSessionDeleteItem = {
  path: string;
  sizeBytes: number;
};

export type AgentSessionsTrashOutcome = CleanOutcome;
