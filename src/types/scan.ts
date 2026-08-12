export type ScanStatus = "running" | "completed" | "cancelled" | "failed";

export type SearchSort =
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
