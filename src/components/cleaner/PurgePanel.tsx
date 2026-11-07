// 项目瘦身 (Project Slimming): find rebuildable build artifacts inside
// development projects and trash them after explicit confirmation.
//
// Contract (docs/disk-cleaner.md):
//   - Discovery is user-triggered and budgeted; results show per project.
//   - Projects modified within the last 7 days are active and locked.
//   - The backend re-discovers on execution and refuses anything that does
//     not match the fresh discovery (size drift, activity, whitelist).

import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  FolderOpen,
  Lock,
  Package,
  RefreshCw,
  Search,
} from "lucide-react";
import { ConfirmDialog } from "../fileManager/ConfirmDialog";
import { discoverPurgeProjects, executePurge } from "../../lib/tauri";
import { formatBytes, formatDate, formatNumber } from "../../lib/format";
import { errorMessage } from "../../lib/errors";
import type {
  ArtifactGroup,
  CleanRequestItem,
  PurgeProject,
  PurgeReport,
} from "../../types/scan";

// ── Local state ───────────────────────────────────────────────────────────────

type ReportState =
  | { status: "idle" }
  | { status: "scanning" }
  | { status: "error"; message: string }
  | { status: "ready"; report: PurgeReport };

type Selection = Map<string, number>; // artifact path → size bytes

// ── Component ─────────────────────────────────────────────────────────────────

export function PurgePanel() {
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
      const report = await discoverPurgeProjects();
      setState({ status: "ready", report });
    } catch (error) {
      setState({ status: "error", message: errorMessage(error, t("purger.loadError")) });
    }
  }, [t]);

  const toggleArtifact = useCallback((artifact: ArtifactGroup, project: PurgeProject) => {
    if (project.isActive || artifact.gitTracked) return;
    setSelected((prev) => {
      const next = new Map(prev);
      if (next.has(artifact.path)) next.delete(artifact.path);
      else next.set(artifact.path, artifact.sizeBytes);
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
      const outcome = await executePurge(items);
      setResult({ succeeded: outcome.succeeded.length, failed: outcome.failed.length });
      setSelected(new Map());
      // Re-scan so sizes and activity reflect the cleanup.
      const report = await discoverPurgeProjects();
      setState({ status: "ready", report });
    } catch (error) {
      setState({ status: "error", message: errorMessage(error, t("purger.executeError")) });
    } finally {
      setExecuting(false);
      setConfirmOpen(false);
    }
  }, [executing, selected, t]);

  const selectedBytes = [...selected.values()].reduce((sum, size) => sum + size, 0);
  const report = state.status === "ready" ? state.report : null;
  const cleanableProjects = report
    ? report.projects.filter((project) => !project.isActive && project.artifacts.length > 0)
    : [];

  return (
    <section className="result-section purge-section" aria-labelledby="purge-title">
      <div className="section-heading compact-heading">
        <h2 id="purge-title">{t("purger.title")}</h2>
        {report && (
          <button type="button" className="icon-button" title={t("purger.rescan")}
            aria-label={t("purger.rescan")} disabled={state.status === "scanning"}
            onClick={() => void scan()}>
            <RefreshCw size={15} />
          </button>
        )}
      </div>
      <p className="cleaner-subtitle text-secondary">{t("purger.description")}</p>

      {state.status === "idle" && (
        <button type="button" className="button-primary purge-scan-button" onClick={() => void scan()}>
          <Search size={14} aria-hidden="true" />
          {t("purger.scan")}
        </button>
      )}
      {state.status === "scanning" && <p className="empty-inline">{t("purger.scanning")}</p>}
      {state.status === "error" && (
        <p className="empty-inline cleaner-error" role="alert">{state.message}</p>
      )}

      {report && (
        <>
          {result && (
            <div className="cleaner-result" role="status">
              <p>{t("purger.resultSummary", {
                succeeded: formatNumber(result.succeeded),
                failed: formatNumber(result.failed),
              })}</p>
            </div>
          )}

          {report.isPartial && <p className="empty-inline">{t("purger.partial")}</p>}

          {report.projects.length === 0 ? (
            <p className="empty-inline">{t("purger.noProjects")}</p>
          ) : (
            <div className="cleanup-list">
              {report.projects.map((project) => (
                <PurgeProjectRow
                  key={project.path}
                  project={project}
                  selected={selected}
                  onToggleArtifact={toggleArtifact}
                />
              ))}
            </div>
          )}

          {cleanableProjects.length > 0 && (
            <div className="purge-actions">
              <button type="button" className="button-primary purge-clean-button"
                disabled={executing || selected.size === 0}
                onClick={() => setConfirmOpen(true)}>
                {executing
                  ? t("purger.executing")
                  : t("purger.cleanSelected", {
                    count: formatNumber(selected.size),
                    size: formatBytes(selectedBytes),
                  })}
              </button>
            </div>
          )}
        </>
      )}

      <ConfirmDialog
        open={confirmOpen}
        title={t("purger.confirmTitle")}
        message={t("purger.confirmBody", {
          count: formatNumber(selected.size),
          size: formatBytes(selectedBytes),
        })}
        confirmLabel={t("purger.confirmLabel")}
        cancelLabel={t("purger.cancelLabel")}
        variant="danger"
        onConfirm={() => void confirmClean()}
        onCancel={() => setConfirmOpen(false)}
      />
    </section>
  );
}

// ── Project row ───────────────────────────────────────────────────────────────

function PurgeProjectRow({
  project, selected, onToggleArtifact,
}: {
  project: PurgeProject;
  selected: Selection;
  onToggleArtifact: (artifact: ArtifactGroup, project: PurgeProject) => void;
}) {
  const { t } = useTranslation();

  return (
    <div className="insight purge-row">
      <div className="insight-head purge-project-head">
        <span className="insight-icon" style={{
          color: "var(--han-color-info)",
          background: "color-mix(in srgb, var(--han-color-info) 14%, transparent)",
        }}>
          {project.isActive ? <Lock size={15} /> : <FolderOpen size={15} />}
        </span>
        <div className="insight-copy">
          <strong>
            {project.name}
            {project.isActive && (
              <span className="cleaner-risk-badge purge-active-badge">
                {t("purger.active")}
              </span>
            )}
          </strong>
          <p className="text-secondary">
            <span title={project.path}>{project.path}</span>
            {" — "}
            {t("purger.projectTotal", {
              size: formatBytes(project.totalBytes),
              time: project.lastActiveAt ? formatDate(project.lastActiveAt) : t("common.missing"),
            })}
          </p>
        </div>
      </div>

      {project.artifacts.length > 0 && (
        <div className="purge-artifacts">
          {project.artifacts.map((artifact) => {
            const checked = selected.has(artifact.path);
            const disabled = project.isActive || artifact.gitTracked;
            return (
              <label key={artifact.path}
                className={`file-row purge-artifact-row${disabled ? " is-disabled" : ""}`}>
                <input type="checkbox" className="cleaner-unit-check" checked={checked}
                  disabled={disabled}
                  onChange={() => onToggleArtifact(artifact, project)}
                  aria-label={t("purger.artifactAria", {
                    group: t(`purger.groups.${artifact.group}` as "purger.title"),
                    name: project.name,
                  })} />
                <span className="purge-artifact-icon" aria-hidden="true"><Package size={14} /></span>
                <div className="file-copy">
                  <strong title={artifact.path}>
                    {t(`purger.groups.${artifact.group}` as "purger.title")}
                    {artifact.gitTracked && (
                      <span className="cleaner-risk-badge purge-git-badge">
                        {t("purger.gitTracked")}
                      </span>
                    )}
                  </strong>
                  <span title={artifact.path}>{artifact.path}</span>
                </div>
                <div className="file-meta">
                  <strong>{formatBytes(artifact.sizeBytes)}</strong>
                  <span>{formatNumber(artifact.fileCount)} {t("purger.filesUnit")}</span>
                </div>
                <span className="purge-rebuild-hint text-secondary">
                  {t(`purger.rebuildHint.${artifact.group}` as "purger.title")}
                </span>
              </label>
            );
          })}
        </div>
      )}

      {project.isActive && project.artifacts.length > 0 && (
        <p className="insight-note">{t("purger.activeHint")}</p>
      )}
    </div>
  );
}
