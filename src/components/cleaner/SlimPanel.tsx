// 应用瘦身 (App Slimming): strip Intel slices from universal apps on Apple
// Silicon. The backend thins a COPY, re-signs ad hoc, verifies, and only
// then moves the original to the Trash — always recoverable.

import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { RefreshCw, ScanSearch, ShieldAlert } from "lucide-react";
import { ConfirmDialog } from "../fileManager/ConfirmDialog";
import { discoverSlimCandidates, executeSlim } from "../../lib/tauri";
import { formatBytes, formatNumber } from "../../lib/format";
import { errorMessage } from "../../lib/errors";
import type { CleanFailure, SlimCandidate, SlimReport } from "../../types/scan";

type ReportState =
  | { status: "idle" }
  | { status: "scanning" }
  | { status: "error"; message: string }
  | { status: "ready"; report: SlimReport };

type Selection = Map<string, number>; // app path → current bytes

export function SlimPanel() {
  const { t } = useTranslation();

  const [state, setState] = useState<ReportState>({ status: "idle" });
  const [selected, setSelected] = useState<Selection>(new Map());
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [executing, setExecuting] = useState(false);
  const [result, setResult] = useState<{ succeeded: number; failed: CleanFailure[] } | null>(null);

  const scan = useCallback(async () => {
    setState({ status: "scanning" });
    setResult(null);
    setSelected(new Map());
    try {
      const report = await discoverSlimCandidates();
      setState({ status: "ready", report });
    } catch (error) {
      setState({ status: "error", message: errorMessage(error, t("slim.loadError")) });
    }
  }, [t]);

  const toggleCandidate = useCallback((candidate: SlimCandidate) => {
    setSelected((prev) => {
      const next = new Map(prev);
      if (next.has(candidate.path)) next.delete(candidate.path);
      else next.set(candidate.path, candidate.currentBytes);
      return next;
    });
  }, []);

  const confirmSlim = useCallback(async () => {
    if (executing || selected.size === 0) return;
    setExecuting(true);
    try {
      const items = [...selected].map(([path, currentBytes]) => ({ path, currentBytes }));
      const outcome = await executeSlim(items);
      setResult({
        succeeded: outcome.succeeded.length,
        failed: outcome.failed.map((failure) => ({
          path: failure.path,
          reason: failure.reason,
        })),
      });
      setSelected(new Map());
      const report = await discoverSlimCandidates();
      setState({ status: "ready", report });
    } catch (error) {
      setState({ status: "error", message: errorMessage(error, t("slim.executeError")) });
    } finally {
      setExecuting(false);
      setConfirmOpen(false);
    }
  }, [executing, selected, t]);

  const report = state.status === "ready" ? state.report : null;
  const selectedBytes = [...selected.values()].reduce((sum, size) => sum + size, 0);

  return (
    <section className="result-section slim-section" aria-labelledby="slim-title">
      <div className="section-heading compact-heading">
        <h2 id="slim-title">{t("slim.title")}</h2>
        {report && (
          <button type="button" className="icon-button" title={t("slim.rescan")}
            aria-label={t("slim.rescan")} disabled={state.status === "scanning"}
            onClick={() => void scan()}>
            <RefreshCw size={15} />
          </button>
        )}
      </div>
      <p className="cleaner-subtitle text-secondary">{t("slim.description")}</p>

      {state.status === "idle" && (
        <button type="button" className="button-primary purge-scan-button" onClick={() => void scan()}>
          <ScanSearch size={14} aria-hidden="true" />
          {t("slim.scan")}
        </button>
      )}
      {state.status === "scanning" && <p className="empty-inline">{t("slim.scanning")}</p>}
      {state.status === "error" && (
        <p className="empty-inline cleaner-error" role="alert">{state.message}</p>
      )}

      {report && (
        <>
          {result && (
            <div className="cleaner-result" role="status">
              <p>{t("slim.resultSummary", {
                succeeded: formatNumber(result.succeeded),
                failed: formatNumber(result.failed.length),
              })}</p>
              {result.failed.length > 0 && (
                <ul>
                  {result.failed.map((failure) => (
                    <li key={failure.path}>
                      <span>{failure.path}</span>
                      <span>{failure.reason}</span>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}

          {!report.platformSupported ? (
            <p className="empty-inline">{t("slim.unsupported")}</p>
          ) : report.isPartial && <p className="empty-inline">{t("slim.partial")}</p>}

          {report.platformSupported && report.candidates.length === 0 ? (
            <p className="empty-inline">{t("slim.empty")}</p>
          ) : report.platformSupported && (
            <>
              <p className="orphans-hint">
                <ShieldAlert size={14} aria-hidden="true" />
                {t("slim.hint")}
              </p>
              <div className="cleanup-list">
                {report.candidates.map((candidate) => (
                  <label key={candidate.path} className="file-row purge-artifact-row slim-row">
                    <input type="checkbox" className="cleaner-unit-check"
                      checked={selected.has(candidate.path)}
                      onChange={() => toggleCandidate(candidate)}
                      aria-label={t("slim.itemAria", { name: candidate.name })} />
                    <div className="file-copy">
                      <strong title={candidate.path}>{candidate.name}</strong>
                      <span title={candidate.path}>{candidate.path}</span>
                    </div>
                    <div className="file-meta">
                      <strong>{formatBytes(candidate.currentBytes)}</strong>
                      <span>{t("slim.saving", { size: formatBytes(candidate.savingBytes) })}</span>
                    </div>
                  </label>
                ))}
              </div>
              {report.candidates.length > 0 && (
                <div className="purge-actions">
                  <button type="button" className="button-primary purge-clean-button"
                    disabled={executing || selected.size === 0}
                    onClick={() => setConfirmOpen(true)}>
                    {executing
                      ? t("slim.executing")
                      : t("slim.cleanSelected", {
                        count: formatNumber(selected.size),
                        size: formatBytes(selectedBytes),
                      })}
                  </button>
                </div>
              )}
            </>
          )}
        </>
      )}

      <ConfirmDialog
        open={confirmOpen}
        title={t("slim.confirmTitle")}
        message={t("slim.confirmBody", {
          count: formatNumber(selected.size),
          size: formatBytes(selectedBytes),
        })}
        confirmLabel={t("slim.confirmLabel")}
        cancelLabel={t("slim.cancelLabel")}
        variant="danger"
        onConfirm={() => void confirmSlim()}
        onCancel={() => setConfirmOpen(false)}
      />
    </section>
  );
}
