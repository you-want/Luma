export type ScanStatus = "running" | "completed" | "cancelled" | "failed";

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
  basis: string;
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
  size_bytes: number;
  file_count: number;
};
