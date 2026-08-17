// Types for the File Manager (Phase 1: browse + preview, Phase 2: operations).
// Kept separate from scan.ts so neither side grows to an unmanageable size.

import type { FileEntry } from "./scan";

/** A directory node derived from the scan index — no live filesystem access. */
export type DirNode = {
  path: string;
  name: string;
  /** Total files anywhere under this directory (recursive). */
  fileCount: number;
  sizeBytes: number;
  /** Whether this directory contains at least one subdirectory. */
  hasChildren: boolean;
};

/** One page of a directory listing: subdirs + direct files + total count. */
export type DirectoryListing = {
  dirs: DirNode[];
  files: FileEntry[];
  /** Total direct files in this directory (for pagination). */
  totalFiles: number;
};

/** Sort tokens for directory file listings. */
export type DirFileSort =
  | "nameAsc"
  | "nameDesc"
  | "sizeDesc"
  | "sizeAsc"
  | "modifiedDesc"
  | "modifiedAsc";

/** A breadcrumb segment: a path and its display name. */
export type BreadcrumbSegment = {
  path: string;
  name: string;
};

/** View mode for the file list panel. */
export type FileListView = "list" | "grid";

// ── Phase 2: File operations ───────────────────────────────────────────────────

/** Outcome of a batch file operation. */
export type OpResult = {
  operationId?: string;
  succeeded: string[];
  failed: OpFailure[];
};

export type OpFailure = {
  path: string;
  reason: string;
};

/** Result of a rename — carries the new path and persistent operation id. */
export type RenameResult = {
  newPath: string;
  operationId: string;
};

/** A write operation whose undo plan is persisted in SQLite. */
export type OperationRecord = {
  id: string;
  kind: string;
  label: string;
  status: string;
  createdAt: number;
  canUndo: boolean;
};

// ── Phase 3: Smart organizer ───────────────────────────────────────────────────

export type OrganizeRule =
  | { kind: "byCategory" }
  | { kind: "byYear" }
  | { kind: "byYearMonth" }
  | { kind: "byExtension" };

export type OrganizeMoveInput = {
  from: string;
  to: string;
  expectedSizeBytes: number;
  expectedModifiedAt?: number;
};

export type OrganizeMove = OrganizeMoveInput & {
  fromName: string;
  subfolder: string;
  conflict: boolean;
};

export type OrganizePlan = {
  ruleName: string;
  sourceDir: string;
  destDir: string;
  moves: OrganizeMove[];
  conflictCount: number;
  alreadyPlaced: number;
};

export type OrganizeProgress = {
  done: number;
  total: number;
  currentFrom: string;
};

export type OrganizeResult = {
  operationId?: string;
  succeeded: number;
  failed: OpFailure[];
};
