// OverviewView — the landing dashboard. Answers "what is worth doing with
// this directory right now" with three action cards (整理 / 清理 / 重复),
// the latest write operations, and a compact scan summary. Every card routes
// into the flow that can actually act, so no entry is a dead end.

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  CheckCircle2,
  ChevronRight,
  Copy,
  Database,
  FolderOpen,
  Layers,
  Monitor,
  RotateCcw,
  TriangleAlert,
  Wand2,
} from "lucide-react";
import {
  findDuplicates,
  getCleanupSummary,
  listDirectoryFiles,
  listRecentOperations,
  undoFileOperation,
} from "../../lib/tauri";
import { appStore, useAppStore } from "../../app/store";
import { useToast } from "../../contexts/ToastContext";
import { errorMessage } from "../../lib/errors";
import { formatBytes, formatDate, formatNumber } from "../../lib/format";
import { ButtonSpinner } from "../shell/ButtonSpinner";
import { ScanProgress } from "../analysis/ScanProgress";
import type { ScanProgress as ScanProgressType, ScanSummary } from "../../types/scan";
import type { AppView } from "../../types/navigation";
import type { OperationLogEntry } from "../../types/fileManager";
import "./Overview.css";

type OverviewProps = {
  result: ScanSummary | null;
  phase: string;
  progress?: ScanProgressType;
  loading: boolean;
  hasSelectedPath: boolean;
  pendingAction: string | null;
  onChoose: () => void;
  onShortcut: (location: "desktop" | "downloads") => void;
  onStart: () => void;
  onNavigate: (view: AppView, anchor?: string) => void;
};

type LooseCountState =
  | { status: "loading" }
  | { status: "ready"; count: number }
  | { status: "error" };

export function OverviewView({
  result,
  phase,
  progress,
  loading,
  hasSelectedPath,
  pendingAction,
  onChoose,
  onShortcut,
  onStart,
  onNavigate,
}: OverviewProps) {
  const { t } = useTranslation();
  const toast = useToast();
  const app = useAppStore();

  const [loose, setLoose] = useState<LooseCountState>({ status: "loading" });
  const [cleanupBytes, setCleanupBytes] = useState<number | null>(null);
  const [recentOps, setRecentOps] = useState<OperationLogEntry[]>([]);
  const [undoingId, setUndoingId] = useState<string | null>(null);

  const scanId = result?.scanId ?? null;

  // Reload the overview data whenever the scan or any on-disk mutation
  // (organize, trash, undo…) changes what the numbers should say.
  useEffect(() => {
    if (!scanId || !result) return;
    let disposed = false;
    setLoose({ status: "loading" });
    listDirectoryFiles(scanId, result.rootPath, { limit: 1 })
      .then((listing) => {
        if (!disposed) setLoose({ status: "ready", count: listing.totalFiles });
      })
      .catch(() => { if (!disposed) setLoose({ status: "error" }); });
    getCleanupSummary(scanId)
      .then((summary) => { if (!disposed) setCleanupBytes(summary.totalBytes); })
      .catch(() => { if (!disposed) setCleanupBytes(null); });
    return () => { disposed = true; };
  }, [scanId, result, app.dataRevision]);

  useEffect(() => {
    let disposed = false;
    listRecentOperations(5)
      .then((entries) => { if (!disposed) setRecentOps(entries); })
      .catch(() => { if (!disposed) setRecentOps([]); });
    return () => { disposed = true; };
  }, [app.dataRevision]);

  async function runDuplicatesFind() {
    if (!scanId) return;
    appStore.setDuplicates({ status: "loading" });
    try {
      const groups = await findDuplicates(scanId);
      appStore.setDuplicates({ status: "ready", groups });
    } catch (error) {
      appStore.setDuplicates({
        status: "error",
        message: errorMessage(error, t("duplicates.findError")),
      });
    }
  }

  async function handleUndo(entry: OperationLogEntry) {
    if (!scanId || undoingId) return;
    setUndoingId(entry.id);
    try {
      const outcome = await undoFileOperation(entry.id, scanId);
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
    } finally {
      setUndoingId(null);
    }
  }

  // ── Welcome / running / failed states (no scan result yet) ────────────────
  if (!result) {
    return (
      <div className="workspace-status-panel"><div className="content">
        {phase === "running" && <ScanProgress progress={progress} />}
        {phase === "failed" && <section className="status-message status-error" role="alert">
          <TriangleAlert size={20} />
          <div><strong>{t("status.failedTitle")}</strong><p>{app.error}</p></div>
        </section>}
        {phase === "cancelled" && <section className="status-message">
          <Database size={20} />
          <div><strong>{t("status.cancelledTitle")}</strong><p>{t("status.cancelledBody")}</p></div>
        </section>}
        {loading && <div className="loading-state">{t("status.loading")}</div>}
        {!loading && phase !== "running" && <section className="workspace-welcome">
          <FolderOpen size={32} aria-hidden="true" />
          <h1>{t(hasSelectedPath ? "workspace.readyTitle" : "workspace.welcomeTitle")}</h1>
          <p>{t(hasSelectedPath ? "workspace.readyBody" : "workspace.welcomeBody")}</p>
          <div className="overview-quick-row" aria-label={t("workspace.quickLocations")}>
            <button type="button" className="button button-secondary" disabled={pendingAction !== null}
              aria-busy={pendingAction === "desktop"} onClick={() => onShortcut("desktop")}>
              {pendingAction === "desktop" ? <ButtonSpinner /> : <Monitor size={14} aria-hidden="true" />}
              {t("scanControls.desktop")}
            </button>
            <button type="button" className="button button-secondary" disabled={pendingAction !== null}
              aria-busy={pendingAction === "downloads"} onClick={() => onShortcut("downloads")}>
              {pendingAction === "downloads" ? <ButtonSpinner /> : <FolderOpen size={14} aria-hidden="true" />}
              {t("scanControls.downloads")}
            </button>
            <button type="button" className="button button-secondary" disabled={pendingAction !== null}
              aria-busy={pendingAction === "choose"} onClick={onChoose}>
              {pendingAction === "choose" ? <ButtonSpinner /> : <FolderOpen size={14} aria-hidden="true" />}
              {t("workspace.choose")}
            </button>
          </div>
          <div className="overview-scan-cta">
            <button type="button" className="button button-primary" disabled={!hasSelectedPath || pendingAction !== null}
              aria-busy={pendingAction === "start"} onClick={onStart}>
              {pendingAction === "start" ? <ButtonSpinner size={15} /> : null}
              {t(pendingAction === "start" ? "scanControls.starting" : "scanControls.scan")}
            </button>
          </div>
          <ol className="workspace-flow" aria-label={t("workspace.flowLabel")}>
            <li><span>1</span>{t("workspace.choose")}</li>
            <li><span>2</span>{t("scanControls.scan")}</li>
            <li><span>3</span>{t("workspace.useViews")}</li>
          </ol>
        </section>}
      </div></div>
    );
  }

  // ── Dashboard (scan result available) ─────────────────────────────────────
  const duplicatesTotal = app.duplicates.status === "ready"
    ? app.duplicates.groups.reduce((sum, g) => sum + g.wastedBytes, 0)
    : 0;

  return (
    <div className="workspace-status-panel overview-panel"><div className="content">
      <header className="overview-header">
        <h1>{t("overviewCards.greeting")}</h1>
        <p>{t("overviewCards.context", {
          path: result.rootPath,
          size: formatBytes(result.totalBytes),
        })}</p>
      </header>

      <div className="overview-cards">
        <section className="overview-card" aria-labelledby="overview-organize">
          <span className="overview-card-icon overview-card-icon--accent"><Wand2 size={16} aria-hidden="true" /></span>
          <h2 id="overview-organize">{t("overviewCards.organizeTitle")}</h2>
          {loose.status === "ready"
            ? <p className="overview-card-value">{t("overviewCards.filesCount", { count: loose.count })}</p>
            : loose.status === "loading"
              ? <p className="overview-card-value"><ButtonSpinner size={14} /></p>
              : <p className="overview-card-value">—</p>}
          <p className="overview-card-sub">{t(loose.status === "ready" && loose.count === 0
            ? "overviewCards.organizeDoneSub" : "overviewCards.organizeSub")}</p>
          {loose.status === "ready" && loose.count === 0 && <CheckCircle2 size={16} className="overview-card-ok" aria-hidden="true" />}
          <button type="button" className="button button-primary overview-card-action"
            onClick={() => { appStore.seedOrganize(result.rootPath); onNavigate("organize"); }}>
            {t("overviewCards.organizeAction")}
            <ChevronRight size={14} aria-hidden="true" />
          </button>
        </section>

        <section className="overview-card" aria-labelledby="overview-cleanup">
          <span className="overview-card-icon overview-card-icon--green"><Layers size={16} aria-hidden="true" /></span>
          <h2 id="overview-cleanup">{t("overviewCards.cleanupTitle")}</h2>
          <p className="overview-card-value">{cleanupBytes === null ? "—" : formatBytes(cleanupBytes)}</p>
          <p className="overview-card-sub">{t("overviewCards.cleanupSub")}</p>
          <button type="button" className="button overview-card-action" disabled={cleanupBytes === null}
            onClick={() => onNavigate("analysis", ".cleanup-section")}>
            {t("overviewCards.cleanupAction")}
            <ChevronRight size={14} aria-hidden="true" />
          </button>
        </section>

        <section className="overview-card" aria-labelledby="overview-duplicates">
          <span className="overview-card-icon overview-card-icon--orange"><Copy size={16} aria-hidden="true" /></span>
          <h2 id="overview-duplicates">{t("overviewCards.dupTitle")}</h2>
          {app.duplicates.status === "ready" && <p className="overview-card-value">
            {app.duplicates.groups.length > 0
              ? t("overviewCards.dupGroups", { count: app.duplicates.groups.length, size: formatBytes(duplicatesTotal) })
              : t("overviewCards.dupNone")}
          </p>}
          {app.duplicates.status !== "ready" && <p className="overview-card-value">
            {app.duplicates.status === "loading"
              ? <ButtonSpinner size={14} />
              : t("overviewCards.dupIdle")}
          </p>}
          <p className="overview-card-sub">{t("overviewCards.dupSub")}</p>
          {app.duplicates.status === "error" && <p className="overview-card-error" role="alert">{app.duplicates.message}</p>}
          {app.duplicates.status === "ready" && app.duplicates.groups.length > 0 ? (
            <button type="button" className="button overview-card-action"
              onClick={() => onNavigate("analysis", ".duplicates-section")}>
              {t("overviewCards.dupAction")}
              <ChevronRight size={14} aria-hidden="true" />
            </button>
          ) : (
            <button type="button" className="button overview-card-action"
              disabled={app.duplicates.status === "loading"}
              onClick={() => void runDuplicatesFind()}>
              {t(app.duplicates.status === "loading" ? "overviewCards.dupFinding" : "overviewCards.dupFind")}
            </button>
          )}
        </section>
      </div>

      <div className="overview-columns">
        <section className="overview-card overview-recent" aria-labelledby="overview-recent-title">
          <div className="overview-recent-head">
            <h2 id="overview-recent-title">{t("overviewCards.recentTitle")}</h2>
            <button type="button" className="button" onClick={() => onNavigate("history")}>
              {t("overviewCards.recentAll")}
            </button>
          </div>
          {recentOps.length === 0 ? <p className="empty-inline">{t("overviewCards.recentEmpty")}</p> : (
            <ul className="overview-op-list">
              {recentOps.map((entry) => {
                const undoableHere = entry.canUndo && entry.rootPath === result.rootPath;
                return (
                  <li className="overview-op-row" key={entry.id}>
                    <span className={`op-kind-badge op-kind-badge--${entry.scanId === "cleaner" ? "machine" : entry.kind}`}>
                      {t(`operationLog.kind.${entry.scanId === "cleaner" ? "machine" : entry.kind}` as "operationLog.kind.organize")}
                    </span>
                    <span className="overview-op-label" title={entry.label}>{entry.label}</span>
                    <time className="overview-op-time">{formatDate(entry.createdAt)}</time>
                    {undoableHere ? (
                      <button type="button" className="button" disabled={undoingId !== null}
                        aria-busy={undoingId === entry.id}
                        onClick={() => void handleUndo(entry)}>
                        {undoingId === entry.id ? <ButtonSpinner size={13} /> : <RotateCcw size={13} aria-hidden="true" />}
                        {t("operationLog.undo")}
                      </button>
                    ) : entry.scanId === "cleaner" ? (
                      <span className="op-recoverable">{t("operationLog.trashRecoverable")}</span>
                    ) : <span className="overview-op-spacer" />}
                  </li>
                );
              })}
            </ul>
          )}
        </section>

        <section className="overview-card" aria-labelledby="overview-summary-title">
          <h2 id="overview-summary-title">{t("overviewCards.summaryTitle")}</h2>
          <dl className="overview-summary-list">
            <div><dt>{t("overview.files")}</dt><dd>{formatNumber(result.totalFiles)}</dd></div>
            <div><dt>{t("overview.directories")}</dt><dd>{formatNumber(result.totalDirectories)}</dd></div>
            <div><dt>{t("overview.totalSize")}</dt><dd>{formatBytes(result.totalBytes)}</dd></div>
            <div><dt>{t("overview.readErrors")}</dt><dd>{formatNumber(result.errorCount)}</dd></div>
          </dl>
          {result.finishedAt && <p className="overview-scan-time">{t("overview.finishedAt", { time: formatDate(result.finishedAt) })}</p>}
        </section>
      </div>
    </div></div>
  );
}
