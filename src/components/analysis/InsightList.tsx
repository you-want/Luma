import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Archive,
  ChevronRight,
  Clock3,
  Code2,
  Package,
  PackageSearch,
  Trash2,
} from "lucide-react";
import { listInsightFiles, revealPath, trashFiles } from "../../lib/tauri";
import { appStore } from "../../app/store";
import { useToast } from "../../contexts/ToastContext";
import { useAsyncAction } from "../../hooks/useAsyncAction";
import { categoryColor, categoryIcon, categoryTint } from "../../lib/categories";
import { formatBytes, formatDate, formatNumber } from "../../lib/format";
import { errorMessage } from "../../lib/errors";
import { ConfirmDialog } from "../fileManager/ConfirmDialog";
import type { FileEntry, InsightKind, InsightSummary } from "../../types/scan";

const insightMeta: Record<InsightKind, { icon: typeof PackageSearch; color: string }> = {
  largeFiles: { icon: PackageSearch, color: "var(--han-color-warning)" },
  staleFiles: { icon: Clock3, color: "var(--luma-category-other)" },
  development: { icon: Code2, color: "var(--han-color-info)" },
  archives: { icon: Archive, color: "var(--luma-category-archives)" },
  installers: { icon: Package, color: "var(--han-color-accent-decorative)" },
};

type InsightListProps = {
  scanId?: string;
  insights: InsightSummary[];
  largeFileThreshold: number;
  staleDays: number;
  onSettingsChange: (largeFileThreshold: number, staleDays: number) => void;
};

type FileState =
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "ready"; files: FileEntry[] };

export function InsightList({
  scanId,
  insights,
  largeFileThreshold,
  staleDays,
  onSettingsChange,
}: InsightListProps) {
  const { t } = useTranslation();
  const toast = useToast();
  const trashAction = useAsyncAction<"trash">();
  const [openKind, setOpenKind] = useState<InsightKind | null>(null);
  const [files, setFiles] = useState<Partial<Record<InsightKind, FileState>>>({});
  const [trashPending, setTrashPending] = useState<FileEntry | null>(null);

  async function confirmTrash() {
    const file = trashPending;
    if (!file || !scanId) return;
    setTrashPending(null);
    await trashAction.run("trash", async () => {
      try {
        const result = await trashFiles(scanId, [file.path]);
        appStore.bumpDataRevision();
        if (result.succeeded.length > 0) {
          toast.push({ kind: "success", message: t("insight.trashDone", { name: file.name }) });
        }
        for (const failure of result.failed) {
          toast.push({ kind: "error", message: t("fileManager.ops.trashItemFailed", { reason: failure.reason }) });
        }
      } catch (error) {
        toast.push({ kind: "error", message: errorMessage(error) });
      }
    });
  }

  // The detail rows depend on the same thresholds as the summary counts, so a
  // rule change invalidates anything already fetched.
  useEffect(() => {
    setOpenKind(null);
    setFiles({});
  }, [scanId, largeFileThreshold, staleDays]);

  // Rebuild the human-readable rule ("basis") on the frontend from the insight
  // kind plus the current thresholds, so the rationale is translatable instead
  // of being a fixed sentence returned by the backend.
  function basisFor(kind: InsightKind): string {
    switch (kind) {
      case "largeFiles":
        return t("insight.basis.largeFiles", { size: formatBytes(largeFileThreshold) });
      case "staleFiles":
        return t("insight.basis.staleFiles", { days: formatNumber(staleDays) });
      default:
        return t(`insight.basis.${kind}`);
    }
  }

  async function toggle(kind: InsightKind) {
    if (openKind === kind) {
      setOpenKind(null);
      return;
    }
    setOpenKind(kind);
    if (!scanId || files[kind]?.status === "ready") return;

    setFiles((prev) => ({ ...prev, [kind]: { status: "loading" } }));
    try {
      const loaded = await listInsightFiles(
        scanId,
        kind,
        largeFileThreshold,
        staleDays,
      );
      setFiles((prev) => ({ ...prev, [kind]: { status: "ready", files: loaded } }));
    } catch (error) {
      setFiles((prev) => ({
        ...prev,
        [kind]: {
          status: "error",
          message: errorMessage(error, t("insight.loadFilesError")),
        },
      }));
    }
  }

  return (
    <section className="result-section insight-section" aria-labelledby="insight-title">
      <div className="section-heading compact-heading">
        <h2 id="insight-title">{t("insight.title")}</h2>
      </div>
      <div className="insight-filters" aria-label={t("insight.filtersLabel")}>
        <label>
          {t("insight.largeFile")}
          <select
            value={largeFileThreshold}
            onChange={(event) => onSettingsChange(Number(event.target.value), staleDays)}
          >
            <option value={256 * 1024 ** 2}>{t("insight.over256mb")}</option>
            <option value={1024 ** 3}>{t("insight.over1gb")}</option>
            <option value={5 * 1024 ** 3}>{t("insight.over5gb")}</option>
          </select>
        </label>
        <label>
          {t("insight.unmodified")}
          <select
            value={staleDays}
            onChange={(event) => onSettingsChange(largeFileThreshold, Number(event.target.value))}
          >
            <option value={90}>{t("insight.over90d")}</option>
            <option value={180}>{t("insight.over180d")}</option>
            <option value={365}>{t("insight.over1y")}</option>
          </select>
        </label>
      </div>
      {insights.length ? (
        <div className="insight-list">
          {insights.map((insight) => {
            const { icon: Icon, color } = insightMeta[insight.kind];
            const expanded = openKind === insight.kind;
            const detail = files[insight.kind];
            return (
              <div className="insight" key={insight.kind}>
                <button
                  className="insight-head"
                  type="button"
                  aria-expanded={expanded}
                  onClick={() => toggle(insight.kind)}
                >
                  <span
                    className="insight-icon"
                    style={{
                      color,
                      background: `color-mix(in srgb, ${color} 12%, transparent)`,
                    }}
                  >
                    <Icon size={16} />
                  </span>
                  <div className="insight-copy">
                    <strong>{t(`insight.${insight.kind}`)}</strong>
                    <p>
                      {t("insight.summary", {
                        count: formatNumber(insight.fileCount),
                        size: formatBytes(insight.sizeBytes),
                        basis: basisFor(insight.kind),
                      })}
                    </p>
                  </div>
                  <ChevronRight
                    className={`insight-chevron${expanded ? " is-open" : ""}`}
                    size={16}
                  />
                </button>
                {expanded && (
                  <div className="insight-files">
                    {detail?.status === "loading" && (
                      <p className="empty-inline">{t("insight.loadingFiles")}</p>
                    )}
                    {detail?.status === "error" && (
                      <p className="empty-inline">{detail.message}</p>
                    )}
                    {detail?.status === "ready" &&
                      (detail.files.length ? (
                        <>
                          <div className="file-list">
                            {detail.files.map((file) => {
                              const FileIcon = categoryIcon(file.category);
                              const tint = categoryColor(file.category);
                              return (
                                <div className="file-row" key={file.path}>
                                  <span
                                    className="file-icon"
                                    style={{
                                      color: tint,
                                      background: categoryTint(file.category),
                                    }}
                                  >
                                    <FileIcon size={16} />
                                  </span>
                                  <div className="file-copy">
                                    <strong title={file.name}>{file.name}</strong>
                                    <span title={file.path}>{file.path}</span>
                                  </div>
                                  <div className="file-meta">
                                    <strong>{formatBytes(file.sizeBytes)}</strong>
                                    <span>{formatDate(file.modifiedAt)}</span>
                                  </div>
                                  <span className="file-actions">
                                    {scanId && (
                                      <button
                                        className="icon-button"
                                        type="button"
                                        title={t("fileManager.list.trashTitle")}
                                        aria-label={t("fileManager.list.trashAria", { name: file.name })}
                                        disabled={trashAction.pending !== null}
                                        onClick={() => setTrashPending(file)}
                                      >
                                        <Trash2 size={14} />
                                      </button>
                                    )}
                                    <button
                                      className="icon-button"
                                      type="button"
                                      title={t("common.reveal")}
                                      aria-label={t("common.revealNamed", { name: file.name })}
                                      onClick={() =>
                                        void revealPath(file.path).catch(() => undefined)
                                      }
                                    >
                                      <ChevronRight size={16} />
                                    </button>
                                  </span>
                                </div>
                              );
                            })}
                          </div>
                          {insight.fileCount > detail.files.length && (
                            <p className="insight-more">
                              {t("insight.showingTop", {
                                shown: formatNumber(detail.files.length),
                                total: formatNumber(insight.fileCount),
                              })}
                            </p>
                          )}
                        </>
                      ) : (
                        <p className="empty-inline">{t("insight.noFiles")}</p>
                      ))}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      ) : (
        <p className="empty-inline">{t("insight.empty")}</p>
      )}
      <ConfirmDialog
        open={trashPending !== null}
        title={t("fileManager.confirmTrash.title")}
        confirmLabel={t("fileManager.confirmTrash.title")}
        variant="danger"
        message={trashPending
          ? t("fileManager.confirmTrash.messageOne", { name: trashPending.name })
          : ""}
        onConfirm={() => void confirmTrash()}
        onCancel={() => setTrashPending(null)}
      />

      <p className="insight-note">{t("insight.note")}</p>
    </section>
  );
}
