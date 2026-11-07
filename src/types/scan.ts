export type ScanStatus = "running" | "completed" | "cancelled" | "failed";

export type SearchSort =
  | "relevance"
  | "nameAsc"
  | "nameDesc"
  | "sizeAsc"
  | "sizeDesc"
  | "modifiedAsc"
  | "modifiedDesc";

export type SearchRequest = {
  scanId: string;
  query: string;
  category?: string;
  extension?: string;
  minSize?: number;
  maxSize?: number;
  modifiedAfter?: number;
  modifiedBefore?: number;
  includeHidden: boolean;
  sort: SearchSort;
  limit?: number;
  offset?: number;
};

export type SearchResponse = {
  files: FileEntry[];
  total: number;
  limit: number;
  offset: number;
};

export type StartScanRequest = {
  rootPath: string;
  includeHidden: boolean;
  stayOnFileSystem: boolean;
};

export type ScanProgress = {
  scanId: string;
  status: ScanStatus;
  filesScanned: number;
  directoriesScanned: number;
  bytesScanned: number;
  errors: number;
  currentPath?: string;
};

export type CategorySummary = {
  category: string;
  fileCount: number;
  sizeBytes: number;
};

export type FileEntry = {
  id: number;
  path: string;
  name: string;
  extension?: string;
  category: string;
  sizeBytes: number;
  modifiedAt?: number;
  isHidden: boolean;
  contentHash?: string;
};

export type ScanSummary = {
  scanId: string;
  rootPath: string;
  status: ScanStatus;
  startedAt: number;
  finishedAt?: number;
  totalFiles: number;
  totalDirectories: number;
  totalBytes: number;
  errorCount: number;
  categories: CategorySummary[];
};

export type InsightKind =
  | "largeFiles"
  | "staleFiles"
  | "development"
  | "archives"
  | "installers";

export type InsightSummary = {
  kind: InsightKind;
  fileCount: number;
  sizeBytes: number;
};

export type ScanFinished = {
  scanId: string;
  status: ScanStatus;
  summary?: ScanSummary;
  error?: { code: string; message: string };
};

export type DuplicateGroup = {
  contentHash: string;
  sizeBytes: number;
  fileCount: number;
  wastedBytes: number;
  files: FileEntry[];
};

export type ProjectKind =
  | "nodejs"
  | "rust"
  | "python"
  | "git"
  | "xcode"
  | "maven"
  | "gradle";

export type ProjectCandidate = {
  path: string;
  name: string;
  kind: ProjectKind;
  sizeBytes: number;
  fileCount: number;
};

export type CleanupItem = {
  kind: string;
  sizeBytes: number;
  fileCount: number;
};

export type CleanupSummary = {
  items: CleanupItem[];
  totalBytes: number;
};

// ── Disk cleaner (磁盘管家) ────────────────────────────────────────────────────

export type CleanerRisk = "safe" | "caution" | "review";

export type CleanerFile = {
  path: string;
  name: string;
  sizeBytes: number;
  modifiedAt: number | null;
};

export type CleanerLocation = {
  kind: string;
  risk: CleanerRisk;
  sizeBytes: number;
  fileCount: number;
  roots: string[];
  isPartial: boolean;
};

export type VolumeInfo = {
  name: string;
  mountPoint: string;
  totalBytes: number;
  usedBytes: number;
  freeBytes: number;
};

export type CleanerOverview = {
  locations: CleanerLocation[];
  volumes: VolumeInfo[];
  generatedAt: number;
};

export type CleanRequestItem = {
  path: string;
  sizeBytes: number;
};

export type CleanFailure = {
  path: string;
  reason: string;
};

export type CleanOutcome = {
  operationId: string | null;
  succeeded: string[];
  failed: CleanFailure[];
};

export type CleanerWhitelistEntry = {
  path: string;
  createdAt: number;
};

/** One cleaner-family run, for the history view (read-only). */
export type CleanerOperation = {
  id: string;
  label: string;
  status: string;
  createdAt: number;
  finishedAt: number | null;
};

// ── App slimming (应用瘦身) ────────────────────────────────────────────────────

export type SlimCandidate = {
  path: string;
  name: string;
  currentBytes: number;
  savingBytes: number;
};

export type SlimReport = {
  candidates: SlimCandidate[];
  isPartial: boolean;
  platformSupported: boolean;
};

export type SlimRequestItem = {
  path: string;
  currentBytes: number;
};

export type SlimOutcome = {
  operationId: string | null;
  succeeded: string[];
  failed: CleanFailure[];
};

// ── System maintenance & status (系统维护 / 系统状态) ──────────────────────────

export type MaintenanceTask = {
  id: string;
  available: boolean;
  needsAdmin: boolean;
};

export type MaintenanceReport = {
  tasks: MaintenanceTask[];
};

export type MaintenanceOutcome = {
  operationId: string | null;
  detail: string;
};

export type BatteryStatus = {
  percent: number;
  charging: boolean;
};

export type SystemStatus = {
  cpuUsagePercent: number | null;
  memoryTotalBytes: number | null;
  memoryUsedBytes: number | null;
  battery: BatteryStatus | null;
  platformSupported: boolean;
};

// ── Project slimming (项目瘦身) ────────────────────────────────────────────────

export type ArtifactGroup = {
  /** Stable group key for labels and rebuild hints. */
  group: string;
  /** Artifact directory path — the deletion unit. */
  path: string;
  sizeBytes: number;
  fileCount: number;
  /** Tracked by git in the enclosing repo — excluded from cleanup. */
  gitTracked: boolean;
};

export type PurgeProject = {
  path: string;
  name: string;
  /** Total project size (may overlap nested projects; display only). */
  totalBytes: number;
  lastActiveAt: number | null;
  /** Modified within the 7-day window — artifacts are not cleanable. */
  isActive: boolean;
  artifacts: ArtifactGroup[];
};

export type PurgeReport = {
  projects: PurgeProject[];
  scannedRoots: string[];
  isPartial: boolean;
};

// ── Orphaned app data (应用残留清理) ───────────────────────────────────────────

export type OrphanItem = {
  /** Leftover path — the deletion unit. */
  path: string;
  /** Directory/file name (e.g. com.company.app). */
  name: string;
  bundleId: string;
  /** caches | savedState | webkit | httpStorages | support | preferences | containers | logs */
  group: string;
  /** "safe" (rebuildable, preselected) | "review" (personal data, opt-in). */
  risk: "safe" | "review";
  sizeBytes: number;
  fileCount: number;
  modifiedAt: number | null;
};

export type AppDataReport = {
  items: OrphanItem[];
  scannedRoots: string[];
  isPartial: boolean;
  platformSupported: boolean;
};

export type CategoryDelta = {
  category: string;
  baseSizeBytes: number;
  targetSizeBytes: number;
  baseFileCount: number;
  targetFileCount: number;
  sizeDelta: number;
  fileCountDelta: number;
};

export type ScanComparison = {
  base: ScanSummary;
  target: ScanSummary;
  totalBytesDelta: number;
  totalFilesDelta: number;
  categories: CategoryDelta[];
};
