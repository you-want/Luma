// HistoryView — the unified operation log (操作记录). One journal for every
// write Luma performed: file-manager operations (rename/move/copy/trash),
// organize runs, and the cleaner/sessions family. Undoable entries expose a
// per-entry undo; trash/cleaner rows point to the system trash instead.

import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { RotateCcw } from "lucide-react";
import { listRecentOperations, undoFileOperation } from "../../lib/tauri";
import { appStore, useAppStore } from "../../app/store";
import { useToast } from "../../contexts/ToastContext";
import { useAsyncAction } from "../../hooks/useAsyncAction";
import { errorMessage } from "../../lib/errors";
import { formatDate } from "../../lib/format";
import { ButtonSpinner } from "../ButtonSpinner";
import type { OperationLogEntry } from "../../types/fileManager";
import "./History.css";

type FilterKey = "all" | "organize" | "files" | "machine";
const FILTERS: readonly FilterKey[] = ["all", "organize", "files", "machine"];

const FILE_KINDS = new Set(["trash", "rename", "move", "copy"]);

function matchesFilter(entry: OperationLogEntry, filter: FilterKey): boolean {
  if (filter === "all") return true;
  if (filter === "organize") return entry.scanId !== "cleaner" && entry.kind === "organize";
  if (filter === "files") return entry.scanId !== "cleaner" && FILE_KINDS.has(entry.kind);
  return entry.scanId === "cleaner";
}

type Props = {
  /** Scan id + root of the currently displayed scan (undo target), or nulls. */
  currentScanId: string | null;
  currentRootPath: string | null;
};

export function HistoryView({ currentScanId, currentRootPath }: Props) {
  const { t } = useTranslation();
  const toast = useToast();
  const app = useAppStore();
  const action = useAsyncAction<"load" | "undo">();
  const [entries, setEntries] = useState<OperationLogEntry[] | null>(null);
  const [filter, setFilter] = useState<FilterKey>("all");
  const [loadError, setLoadError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoadError(null);
    try {
      setEntries(await listRecentOperations(100));
    } catch (error) {
      setLoadError(errorMessage(error, t("operationLog.loadError")));
    }
  }, [t]);

  useEffect(() => { void load(); }, [load]);

  // Reload when anything on disk changed (this page bumps the revision after
  // its own undo as well). Skip the initial mount — `load` already ran.
  const initialRevision = useRef(app.dataRevision);
  useEffect(() => {
    if (app.dataRevision === initialRevision.current) return;
    void load();
  }, [app.dataRevision, load]);

  async function handleUndo(entry: OperationLogEntry) {
    // The undo command validates the operation's root against the ACTIVE scan
    // and updates that scan's index — so always pass the current scan id, and
    // only offer undo when the entry belongs to the same root.
    if (action.pending !== null || !currentScanId || !currentRootPath) return;
    if (entry.rootPath !== currentRootPath) return;
    await action.run("undo", async () => {
      try {
        const outcome = await undoFileOperation(entry.id, currentScanId);
        appStore.bumpDataRevision();
        if (outcome.failed.length === 0) {
          toast.push({ kind: "info", message: t("fileManager.ops.undoDone", { label: entry.label }) });
        } else {
          toast.push({
            kind: "error",
            message: t("fileManager.ops.undoPartialFailed", {
              reason: outcome.failed[0]?.reason ?? t("fileManager.ops.unknownError"),
            }),
          });
        }
      } catch (error) {
        toast.push({ kind: "error", message: t("fileManager.ops.undoFailed", { reason: errorMessage(error) }) });
      }
    });
  }

  const visible = (entries ?? []).filter((entry) => matchesFilter(entry, filter));
  const canUndoHere = (entry: OperationLogEntry) =>
    entry.canUndo && currentRootPath !== null && entry.rootPath === currentRootPath;

  return (
    <div className="workspace-status-panel history-panel"><div className="content">
      <div className="history-filters" role="group" aria-label={t("operationLog.filtersLabel")}>
        {FILTERS.map((key) => (
          <button key={key} type="button"
            className={`history-filter${filter === key ? " history-filter--active" : ""}`}
            aria-pressed={filter === key}
            onClick={() => setFilter(key)}>
            {t(`operationLog.filters.${key}`)}
          </button>
        ))}
      </div>

      {loadError && <p className="status-message status-error" role="alert">{loadError}</p>}
      {action.pending === "load" && <p className="empty-inline"><ButtonSpinner /> {t("status.loading")}</p>}

      {entries !== null && !loadError && (
        visible.length === 0
          ? <p className="empty-inline">{entries.length === 0 ? t("operationLog.empty") : t("operationLog.emptyFiltered")}</p>
          : <ol className="history-list">
            {visible.map((entry) => (
              <li className={`history-row${entry.status === "undone" ? " history-row--undone" : ""}`} key={entry.id}>
                <span className={`history-status history-status--${entry.status}`} aria-hidden="true" />
                <time className="history-time">{formatDate(entry.createdAt)}</time>
                <span className={`op-kind-badge op-kind-badge--${entry.scanId === "cleaner" ? "machine" : entry.kind}`}>
                  {t(`operationLog.kind.${entry.scanId === "cleaner" ? "machine" : entry.kind}` as "operationLog.kind.organize")}
                </span>
                <span className="history-label" title={entry.label}>{entry.label}</span>
                <span className={`history-state history-state--${entry.status}`}>
                  {t(`operationLog.status.${entry.status}` as "operationLog.status.completed")}
                </span>
                {canUndoHere(entry) ? (
                  <button type="button" className="button history-undo" disabled={action.pending !== null}
                    aria-busy={action.pending === "undo"}
                    onClick={() => void handleUndo(entry)}>
                    {action.pending === "undo" ? <ButtonSpinner size={13} /> : <RotateCcw size={13} aria-hidden="true" />}
                    {t("operationLog.undo")}
                  </button>
                ) : entry.scanId === "cleaner" ? (
                  <span className="op-recoverable">{t("operationLog.trashRecoverable")}</span>
                ) : entry.canUndo ? (
                  <span className="op-recoverable op-recoverable--muted"
                    title={t("operationLog.undoNeedsScan")}>{t("operationLog.undoNeedsScanShort")}</span>
                ) : <span className="history-spacer" />}
              </li>
            ))}
          </ol>
      )}

      <p className="insight-note">{t("operationLog.note")}</p>
    </div></div>
  );
}
