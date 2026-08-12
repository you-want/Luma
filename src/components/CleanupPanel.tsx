import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Archive,
  ChevronRight,
  Code2,
  Copy,
  Download,
  FolderOpen,
  Package,
  Trash2,
} from "lucide-react";
import { getCleanupSummary, listCleanupFiles, revealPath } from "../lib/tauri";
import { formatBytes, formatDate, formatNumber } from "../lib/format";
import { categoryColor, categoryIcon, categoryTint } from "../lib/categories";
import { errorMessage } from "../lib/errors";
import type { CleanupItem, CleanupSummary, FileEntry } from "../types/scan";

// ── Icon map ──────────────────────────────────────────────────────────────────

const KIND_ICON: Record<string, typeof Trash2> = {
  trash: Trash2,
  oldDownloads: Download,
  development: Code2,
  archives: Archive,
  installers: Package,
  duplicatesEstimate: Copy,
};

const KIND_COLOR: Record<string, string> = {
  trash: "var(--han-color-error)",
  oldDownloads: "var(--han-color-warning)",
  development: "var(--han-color-info)",
  archives: "var(--luma-category-archives)",
  installers: "var(--han-color-accent-decorative)",
  duplicatesEstimate: "var(--han-color-warning)",
};

// ── Props ─────────────────────────────────────────────────────────────────────

type Props = {
  scanId: string;
};

// ── File detail state ─────────────────────────────────────────────────────────

type FileState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "ready"; files: FileEntry[]; total: number };

// ── Component ─────────────────────────────────────────────────────────────────

export function CleanupPanel({ scanId }: Props) {
  const { t } = useTranslation();

  const [summary, setSummary] = useState<CleanupSummary | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [expandedKind, setExpandedKind] = useState<string | null>(null);
  const [files, setFiles] = useState<Partial<Record<string, FileState>>>({});

  // Reset when the scan changes.
  useEffect(() => {
    setSummary(null);
    setLoadError(null);
    setExpandedKind(null);
    setFiles({});

    getCleanupSummary(scanId)
      .then(setSummary)
      .catch((err) => setLoadError(errorMessage(err, t("cleanup.loadError"))));
  }, [scanId, t]);

  const toggleKind = useCallback(
    (kind: string) => {
      if (expandedKind === kind) {
        setExpandedKind(null);
        return;
      }
      setExpandedKind(kind);

      // Already fetched — don't re-fetch.
      if (files[kind] && files[kind]!.status !== "idle") return;

      setFiles((prev) => ({ ...prev, [kind]: { status: "loading" } }));
      listCleanupFiles(scanId, kind, 20)
        .then((rows) => {
          setFiles((prev) => ({
            ...prev,
            [kind]: { status: "ready", files: rows, total: rows.length },
          }));
        })
        .catch((err) => {
          setFiles((prev) => ({
            ...prev,
            [kind]: {
              status: "error",
              message: errorMessage(err, t("cleanup.loadFilesError")),
            },
          }));
        });
    },
    [expandedKind, files, scanId, t],
  );

  // ── Render ──────────────────────────────────────────────────────────────────

  if (loadError) {
    return (
      <section className="result-section" aria-labelledby="cleanup-title">
        <div className="section-heading compact-heading">
          <h2 id="cleanup-title">{t("cleanup.title")}</h2>
        </div>
        <p className="empty-inline">{loadError}</p>
      </section>
    );
  }

  if (!summary) {
    return (
      <section className="result-section" aria-labelledby="cleanup-title">
        <div className="section-heading compact-heading">
          <h2 id="cleanup-title">{t("cleanup.title")}</h2>
        </div>
        <p className="empty-inline">{t("cleanup.loadingFiles")}</p>
      </section>
    );
  }

  return (
    <section className="result-section cleanup-section" aria-labelledby="cleanup-title">
      <div className="section-heading compact-heading">
        <h2 id="cleanup-title">{t("cleanup.title")}</h2>
        {summary.totalBytes > 0 && (
          <span className="text-secondary">
            {t("cleanup.subtitle", { size: formatBytes(summary.totalBytes) })}
          </span>
        )}
      </div>

      {summary.items.length === 0 ? (
        <p className="empty-inline">{t("cleanup.empty")}</p>
      ) : (
        <div className="cleanup-list">
          {summary.items.map((item) => (
            <CleanupRow
              key={item.kind}
              item={item}
              expanded={expandedKind === item.kind}
              fileState={files[item.kind] ?? { status: "idle" }}
              onToggle={toggleKind}
            />
          ))}
        </div>
      )}

      <p className="insight-note">{t("cleanup.note")}</p>
    </section>
  );
}

// ── Row ───────────────────────────────────────────────────────────────────────

type RowProps = {
  item: CleanupItem;
  expanded: boolean;
  fileState: FileState;
  onToggle: (kind: string) => void;
};

function CleanupRow({ item, expanded, fileState, onToggle }: RowProps) {
  const { t } = useTranslation();
  const Icon = KIND_ICON[item.kind] ?? FolderOpen;
  const color = KIND_COLOR[item.kind] ?? "var(--han-color-info)";

  // Dynamic i18n keys built from item.kind — cast through string to bypass
  // the exhaustive-key constraint; the keys always exist at runtime because
  // kind values are a closed enum from the backend.
  const kindLabel = t(`cleanup.kinds.${item.kind}` as "cleanup.title");
  const basis = String(t(`cleanup.basis.${item.kind}` as "cleanup.title", { days: 180 } as never));

  return (
    <div className="insight cleanup-row">
      <button
        type="button"
        className="insight-head"
        aria-expanded={expanded}
        onClick={() => onToggle(item.kind)}
      >
        <span
          className="insight-icon"
          style={{ color, background: `color-mix(in srgb, ${color} 14%, transparent)` }}
        >
          <Icon size={15} />
        </span>
        <div className="insight-copy">
          <strong>{kindLabel}</strong>
          <p className="text-secondary">
            {t("cleanup.summary", {
              count: formatNumber(item.fileCount),
              size: formatBytes(item.sizeBytes),
            })}
            {" — "}
            {basis}
          </p>
        </div>
        <ChevronRight
          className={`insight-chevron${expanded ? " is-open" : ""}`}
          size={16}
        />
      </button>

      {expanded && (
        <div className="insight-files">
          <CleanupFileList fileState={fileState} total={item.fileCount} />
        </div>
      )}
    </div>
  );
}

// ── File list ─────────────────────────────────────────────────────────────────

type FileListProps = {
  fileState: FileState;
  total: number;
};

function CleanupFileList({ fileState, total }: FileListProps) {
  const { t } = useTranslation();

  if (fileState.status === "idle" || fileState.status === "loading") {
    return <p className="empty-inline">{t("cleanup.loadingFiles")}</p>;
  }
  if (fileState.status === "error") {
    return <p className="empty-inline">{fileState.message}</p>;
  }
  if (fileState.files.length === 0) {
    return <p className="empty-inline">{t("cleanup.noFiles")}</p>;
  }

  return (
    <div className="file-list">
      {fileState.files.map((file) => (
        <CleanupFileRow key={file.path} file={file} />
      ))}
      {total > fileState.files.length && (
        <p className="insight-note">
          {t("cleanup.showingTop", {
            shown: formatNumber(fileState.files.length),
            total: formatNumber(total),
          })}
        </p>
      )}
    </div>
  );
}

// ── File row ──────────────────────────────────────────────────────────────────

function CleanupFileRow({ file }: { file: FileEntry }) {
  const { t } = useTranslation();
  const FileIcon = categoryIcon(file.category);
  const tint = categoryColor(file.category);

  return (
    <div className="file-row">
      <span
        className="file-icon"
        style={{ color: tint, background: categoryTint(file.category) }}
      >
        <FileIcon size={16} />
      </span>
      <div className="file-copy">
        <strong title={file.name}>{file.name}</strong>
        <span title={file.path}>{file.path}</span>
      </div>
      <div className="file-meta">
        <strong>{formatBytes(file.sizeBytes)}</strong>
        <span>{file.modifiedAt ? formatDate(file.modifiedAt) : t("common.missing")}</span>
      </div>
      <button
        type="button"
        className="icon-button"
        title={t("common.reveal")}
        aria-label={t("common.revealNamed", { name: file.name })}
        onClick={() => void revealPath(file.path).catch(() => undefined)}
      >
        <ChevronRight size={16} />
      </button>
    </div>
  );
}
