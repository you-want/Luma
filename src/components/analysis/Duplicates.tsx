import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Copy, ChevronRight, AlertCircle, Trash2 } from "lucide-react";
import { findDuplicates, revealPath, trashFiles } from "../../lib/tauri";
import { appStore, useAppStore } from "../../app/store";
import { useToast } from "../../contexts/ToastContext";
import { useAsyncAction } from "../../hooks/useAsyncAction";
import { categoryColor, categoryIcon, categoryTint } from "../../lib/categories";
import { formatBytes, formatDate, formatNumber } from "../../lib/format";
import { errorMessage } from "../../lib/errors";
import { ConfirmDialog } from "../fileManager/ConfirmDialog";
import type { DuplicateGroup, FileEntry } from "../../types/scan";

type DuplicatesProps = {
  scanId: string;
};

export function Duplicates({ scanId }: DuplicatesProps) {
  const { t } = useTranslation();
  const toast = useToast();
  const app = useAppStore();
  const [minSize, setMinSize] = useState(1024 * 1024); // default 1MB
  const [expandedHash, setExpandedHash] = useState<string | null>(null);
  const [trashPending, setTrashPending] = useState<{ paths: string[]; keepName: string; count: number; size: number } | null>(null);
  const action = useAsyncAction<"trash">();

  async function handleFind() {
    appStore.setDuplicates({ status: "loading" });
    try {
      const groups = await findDuplicates(scanId, minSize);
      appStore.setDuplicates({ status: "ready", groups });
    } catch (error) {
      appStore.setDuplicates({
        status: "error",
        message: errorMessage(error, t("duplicates.findError")),
      });
    }
  }

  // A duplicate group is resolved by keeping its newest copy and trashing the
  // rest. Ties keep the first file in backend order (stable).
  function keepNewest(group: DuplicateGroup): { keep: FileEntry; rest: FileEntry[] } {
    const sorted = [...group.files].sort(
      (a, b) => (b.modifiedAt ?? 0) - (a.modifiedAt ?? 0),
    );
    const [keep, ...rest] = sorted;
    return { keep, rest };
  }

  function toggleGroup(hash: string) {
    setExpandedHash(expandedHash === hash ? null : hash);
  }

  async function confirmTrash() {
    if (!trashPending) return;
    const request = trashPending;
    setTrashPending(null);
    await action.run("trash", async () => {
      try {
        const result = await trashFiles(scanId, request.paths);
        appStore.bumpDataRevision();
        if (result.succeeded.length > 0) {
          toast.push({
            kind: "success",
            message: t("duplicates.trashDone", { count: result.succeeded.length }),
          });
        }
        for (const failure of result.failed) {
          toast.push({ kind: "error", message: t("fileManager.ops.trashItemFailed", { reason: failure.reason }) });
        }
        // Refresh the groups from the index so resolved groups disappear.
        await handleFind();
      } catch (error) {
        toast.push({ kind: "error", message: errorMessage(error) });
      }
    });
  }

  const state = app.duplicates;

  return (
    <section className="result-section duplicates-section" aria-labelledby="duplicates-title">
      <div className="section-heading compact-heading">
        <h2 id="duplicates-title">{t("duplicates.title")}</h2>
      </div>

      <div className="duplicates-controls">
        <label>
          {t("duplicates.minSize")}
          <select value={minSize} onChange={(e) => setMinSize(Number(e.target.value))}>
            <option value={1024 * 1024}>1 MB</option>
            <option value={10 * 1024 * 1024}>10 MB</option>
            <option value={100 * 1024 * 1024}>100 MB</option>
            <option value={1024 * 1024 * 1024}>1 GB</option>
          </select>
        </label>
        <button
          type="button"
          className="primary-button"
          onClick={handleFind}
          disabled={state.status === "loading"}
        >
          {state.status === "loading" ? t("duplicates.finding") : t("duplicates.find")}
        </button>
      </div>

      {state.status === "loading" && (
        <p className="empty-inline">{t("duplicates.findingBody")}</p>
      )}

      {state.status === "error" && (
        <div className="status-message status-error" role="alert">
          <AlertCircle size={20} />
          <div>
            <strong>{t("duplicates.findFailedTitle")}</strong>
            <p>{state.message}</p>
          </div>
        </div>
      )}

      {state.status === "ready" &&
        (state.groups.length > 0 ? (
          <>
            <div className="duplicates-summary">
              <p>
                {t("duplicates.foundSummary", {
                  groups: formatNumber(state.groups.length),
                  size: formatBytes(
                    state.groups.reduce((sum, g) => sum + g.wastedBytes, 0),
                  ),
                })}
              </p>
            </div>
            <div className="duplicate-list">
              {state.groups.map((group) => {
                const expanded = expandedHash === group.contentHash;
                const { keep, rest } = keepNewest(group);
                return (
                  <div className="duplicate-group" key={group.contentHash}>
                    <button
                      className="duplicate-head"
                      type="button"
                      aria-expanded={expanded}
                      onClick={() => toggleGroup(group.contentHash)}
                    >
                      <span
                        className="duplicate-icon"
                        style={{ color: "var(--han-color-warning)" }}
                      >
                        <Copy size={16} />
                      </span>
                      <div className="duplicate-copy">
                        <strong>
                          {t("duplicates.groupSummary", {
                            count: formatNumber(group.fileCount),
                            size: formatBytes(group.sizeBytes),
                          })}
                        </strong>
                        <p>{t("duplicates.canSave", { size: formatBytes(group.wastedBytes) })}</p>
                      </div>
                      <ChevronRight
                        className={`duplicate-chevron${expanded ? " is-open" : ""}`}
                        size={16}
                      />
                    </button>
                    {expanded && (
                      <div className="duplicate-files">
                        <div className="duplicate-group-actions">
                          <p>{t("duplicates.keepNewestHint", {
                            name: keep.name,
                            count: rest.length,
                            size: formatBytes(rest.reduce((sum, f) => sum + f.sizeBytes, 0)),
                          })}</p>
                          <button
                            type="button"
                            className="button button-danger duplicates-trash-button"
                            disabled={action.pending !== null}
                            aria-busy={action.pending === "trash"}
                            onClick={() => setTrashPending({
                              paths: rest.map((f) => f.path),
                              keepName: keep.name,
                              count: rest.length,
                              size: rest.reduce((sum, f) => sum + f.sizeBytes, 0),
                            })}
                          >
                            {action.pending === "trash" ? null : <Trash2 size={14} aria-hidden="true" />}
                            {t("duplicates.keepNewestAction", { count: rest.length })}
                          </button>
                        </div>
                        <div className="file-list">
                          {group.files.map((file) => (
                            <DuplicateFileRow key={file.path} file={file} isNewest={file.path === keep.path} />
                          ))}
                        </div>
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          </>
        ) : (
          <p className="empty-inline">{t("duplicates.notFound")}</p>
        ))}

      <ConfirmDialog
        open={trashPending !== null}
        title={t("duplicates.trashConfirmTitle")}
        confirmLabel={t("duplicates.trashConfirmConfirm")}
        variant="danger"
        message={trashPending && <>
          <p>{t("duplicates.trashConfirmBody", {
            count: trashPending.count,
            size: formatBytes(trashPending.size),
          })}</p>
          <p>{t("duplicates.trashConfirmKeep", { name: trashPending.keepName })}</p>
        </>}
        onConfirm={() => void confirmTrash()}
        onCancel={() => setTrashPending(null)}
      />

      <p className="insight-note">{t("duplicates.note")}</p>
    </section>
  );
}

function DuplicateFileRow({ file, isNewest }: { file: FileEntry; isNewest: boolean }) {
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
        {isNewest && <span className="duplicates-keep-badge">{t("duplicates.keepBadge")}</span>}
        <strong>{formatBytes(file.sizeBytes)}</strong>
        <span>{file.modifiedAt ? formatDate(file.modifiedAt) : t("common.missing")}</span>
      </div>
      <button
        className="icon-button"
        type="button"
        title={t("common.reveal")}
        aria-label={t("common.revealNamed", { name: file.name })}
        onClick={() => void revealPath(file.path).catch(() => undefined)}
      >
        <ChevronRight size={16} />
      </button>
    </div>
  );
}
