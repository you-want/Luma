// 应用残留清理 (Orphaned app data): data left behind by apps the user has
// already removed.
//
// Contract (docs/disk-cleaner.md):
//   - Apps still installed never appear — the backend excludes any bundle id
//     claimed by an installed .app.
//   - "safe" groups (caches/state) start checked; "review" groups (personal
//     data) start unchecked — personal data is kept unless explicitly chosen.
//   - The backend re-discovers on execution and refuses anything that does
//     not match the fresh discovery (size drift, whitelist, protection).

import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { PackageX, RefreshCw, Search, ShieldAlert } from "lucide-react";
import { ConfirmDialog } from "../fileManager/ConfirmDialog";
import { discoverOrphanAppData, executeOrphanClean } from "../../lib/tauri";
import { formatBytes, formatDate, formatNumber } from "../../lib/format";
import { errorMessage } from "../../lib/errors";
import type { AppDataReport, CleanRequestItem, OrphanItem } from "../../types/scan";

// ── Local state ───────────────────────────────────────────────────────────────

type ReportState =
  | { status: "idle" }
  | { status: "scanning" }
  | { status: "error"; message: string }
  | { status: "ready"; report: AppDataReport };

type Selection = Map<string, number>; // leftover path → size bytes

// ── Component ─────────────────────────────────────────────────────────────────

export function OrphanPanel() {
  const { t } = useTranslation();

  const [state, setState] = useState<ReportState>({ status: "idle" });
  const [selected, setSelected] = useState<Selection>(new Map());
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [executing, setExecuting] = useState(false);
  const [result, setResult] = useState<{ succeeded: number; failed: number } | null>(null);

  const scan = useCallback(async () => {
    setState({ status: "scanning" });
    setResult(null);
    setSelected(new Map());
    try {
      const report = await discoverOrphanAppData();
      setState({ status: "ready", report });
      // "safe" rebuildable leftovers start selected; personal data does not.
      const initial: Selection = new Map();
      for (const item of report.items) {
        if (item.risk === "safe") initial.set(item.path, item.sizeBytes);
      }
      setSelected(initial);
    } catch (error) {
      setState({ status: "error", message: errorMessage(error, t("orphans.loadError")) });
    }
  }, [t]);

  const toggleItem = useCallback((item: OrphanItem) => {
    setSelected((prev) => {
      const next = new Map(prev);
      if (next.has(item.path)) next.delete(item.path);
      else next.set(item.path, item.sizeBytes);
      return next;
    });
  }, []);

  const confirmClean = useCallback(async () => {
    if (executing || selected.size === 0) return;
    setExecuting(true);
    try {
      const items: CleanRequestItem[] = [...selected].map(([path, sizeBytes]) => ({
        path,
        sizeBytes,
      }));
      const outcome = await executeOrphanClean(items);
      setResult({ succeeded: outcome.succeeded.length, failed: outcome.failed.length });
      setSelected(new Map());
      // Re-scan so the list reflects the cleanup.
      const report = await discoverOrphanAppData();
      setState({ status: "ready", report });
    } catch (error) {
      setState({ status: "error", message: errorMessage(error, t("orphans.executeError")) });
    } finally {
      setExecuting(false);
      setConfirmOpen(false);
    }
  }, [executing, selected, t]);

  const report = state.status === "ready" ? state.report : null;
  const selectedBytes = [...selected.values()].reduce((sum, size) => sum + size, 0);

  return (
    <section className="result-section orphans-section" aria-labelledby="orphans-title">
      <div className="section-heading compact-heading">
        <h2 id="orphans-title">{t("orphans.title")}</h2>
        {report && (
          <button type="button" className="icon-button" title={t("orphans.rescan")}
            aria-label={t("orphans.rescan")} disabled={state.status === "scanning"}
            onClick={() => void scan()}>
            <RefreshCw size={15} />
          </button>
        )}
      </div>
      <p className="cleaner-subtitle text-secondary">{t("orphans.description")}</p>

      {state.status === "idle" && (
        <button type="button" className="button-primary purge-scan-button" onClick={() => void scan()}>
          <Search size={14} aria-hidden="true" />
          {t("orphans.scan")}
        </button>
      )}
      {state.status === "scanning" && <p className="empty-inline">{t("orphans.scanning")}</p>}
      {state.status === "error" && (
        <p className="empty-inline cleaner-error" role="alert">{state.message}</p>
      )}

      {report && (
        <>
          {result && (
            <div className="cleaner-result" role="status">
              <p>{t("orphans.resultSummary", {
                succeeded: formatNumber(result.succeeded),
                failed: formatNumber(result.failed),
              })}</p>
            </div>
          )}

          {!report.platformSupported ? (
            <p className="empty-inline">{t("orphans.unsupported")}</p>
          ) : report.isPartial && <p className="empty-inline">{t("orphans.partial")}</p>}

          {report.platformSupported && report.items.length === 0 ? (
            <p className="empty-inline">{t("orphans.empty")}</p>
          ) : report.platformSupported && (
            <>
              <p className="orphans-hint">
                <ShieldAlert size={14} aria-hidden="true" />
                {t("orphans.hint")}
              </p>
              <div className="cleanup-list">
                {report.items.map((item) => (
                  <OrphanRow key={item.path} item={item} checked={selected.has(item.path)}
                    onToggle={() => toggleItem(item)} />
                ))}
              </div>
              {report.items.length > 0 && (
                <div className="purge-actions">
                  <button type="button" className="button-primary purge-clean-button"
                    disabled={executing || selected.size === 0}
                    onClick={() => setConfirmOpen(true)}>
                    {executing
                      ? t("orphans.executing")
                      : t("orphans.cleanSelected", {
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
        title={t("orphans.confirmTitle")}
        message={t("orphans.confirmBody", {
          count: formatNumber(selected.size),
          size: formatBytes(selectedBytes),
        })}
        confirmLabel={t("orphans.confirmLabel")}
        cancelLabel={t("orphans.cancelLabel")}
        variant="danger"
        onConfirm={() => void confirmClean()}
        onCancel={() => setConfirmOpen(false)}
      />
    </section>
  );
}

// ── Leftover row ──────────────────────────────────────────────────────────────

function OrphanRow({
  item, checked, onToggle,
}: {
  item: OrphanItem;
  checked: boolean;
  onToggle: () => void;
}) {
  const { t } = useTranslation();
  const riskColor = item.risk === "safe" ? "var(--han-color-accent)" : "var(--han-color-warning)";

  return (
    <label className="file-row purge-artifact-row orphans-row">
      <input type="checkbox" className="cleaner-unit-check" checked={checked} onChange={onToggle}
        aria-label={t("orphans.itemAria", {
          group: t(`orphans.groups.${item.group}` as "orphans.title"),
          name: item.bundleId,
        })} />
      <span className="purge-artifact-icon" aria-hidden="true"><PackageX size={14} /></span>
      <div className="file-copy">
        <strong title={item.path}>
          {item.bundleId}
          <span className="cleaner-risk-badge" style={{
            color: riskColor,
            background: `color-mix(in srgb, ${riskColor} 12%, transparent)`,
          }}>{t(`orphans.groups.${item.group}` as "orphans.title")}</span>
        </strong>
        <span title={item.path}>{item.path}</span>
      </div>
      <div className="file-meta">
        <strong>{formatBytes(item.sizeBytes)}</strong>
        <span>{item.modifiedAt ? formatDate(item.modifiedAt) : t("common.missing")}</span>
      </div>
    </label>
  );
}
