// 系统维护 (System Maintenance): QuickLook cache refresh and DNS flush.
// Privileged tasks (DNS) make the OS show its native authorization dialog.

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Droplets, Gauge, Wrench } from "lucide-react";
import { ConfirmDialog } from "../fileManager/ConfirmDialog";
import { getMaintenanceTasks, runMaintenanceTask } from "../../lib/tauri";
import { errorMessage } from "../../lib/errors";
import type { MaintenanceReport } from "../../types/scan";

type Outcome = { taskId: string; detail: string } | null;

const TASK_META: Record<string, { nameKey: string; descKey: string; icon: typeof Wrench }> = {
  quicklookCache: { nameKey: "maintenance.quicklookCache.name", descKey: "maintenance.quicklookCache.description", icon: Gauge },
  dnsFlush: { nameKey: "maintenance.dnsFlush.name", descKey: "maintenance.dnsFlush.description", icon: Droplets },
};

export function MaintenancePanel() {
  const { t } = useTranslation();
  const [report, setReport] = useState<MaintenanceReport | null>(null);
  const [confirmTask, setConfirmTask] = useState<string | null>(null);
  const [running, setRunning] = useState<string | null>(null);
  const [outcome, setOutcome] = useState<Outcome>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getMaintenanceTasks()
      .then(setReport)
      .catch((err) => setError(errorMessage(err, t("maintenance.loadError"))));
  }, [t]);

  const run = useCallback(async () => {
    const task = confirmTask;
    if (!task || running) return;
    setRunning(task);
    setError(null);
    setOutcome(null);
    try {
      const result = await runMaintenanceTask(task);
      setOutcome({ taskId: task, detail: result.detail });
    } catch (err) {
      setError(errorMessage(err, t("maintenance.failed")));
    } finally {
      setRunning(null);
      setConfirmTask(null);
    }
  }, [confirmTask, running, t]);

  return (
    <section className="result-section maintenance-section" aria-labelledby="maintenance-title">
      <div className="section-heading compact-heading">
        <h2 id="maintenance-title">{t("maintenance.title")}</h2>
      </div>
      <p className="cleaner-subtitle text-secondary">{t("maintenance.description")}</p>

      {error && <p className="empty-inline cleaner-error" role="alert">{error}</p>}
      {outcome && (
        <div className="cleaner-result" role="status">
          <p>
            {outcome.taskId === "quicklookCache"
              ? t("maintenance.quicklookCache.name")
              : t("maintenance.dnsFlush.name")}
            {" — "}
            {outcome.detail}
          </p>
        </div>
      )}

      <div className="maintenance-task-list">
        {(report?.tasks ?? []).map((task) => {
          const meta = TASK_META[task.id];
          const Icon = meta?.icon ?? Wrench;
          const busy = running === task.id;
          return (
            <div key={task.id} className="maintenance-task">
              <span className="insight-icon" style={{
                color: "var(--han-color-info)",
                background: "color-mix(in srgb, var(--han-color-info) 14%, transparent)",
              }}>
                <Icon size={15} />
              </span>
              <div className="file-copy">
                <strong>
                  {meta ? t(meta.nameKey as "maintenance.title") : task.id}
                  {task.needsAdmin && (
                    <span className="cleaner-risk-badge maintenance-admin-badge">
                      {t("maintenance.needsAdmin")}
                    </span>
                  )}
                </strong>
                <p className="text-secondary">
                  {meta ? t(meta.descKey as "maintenance.title") : ""}
                </p>
              </div>
              {task.available && (
                <button type="button" className="cleaner-add-button" disabled={running !== null}
                  onClick={() => setConfirmTask(task.id)}>
                  {busy ? t("maintenance.running") : t("maintenance.run")}
                </button>
              )}
            </div>
          );
        })}
      </div>

      <ConfirmDialog
        open={confirmTask !== null}
        title={t("maintenance.confirmTitle")}
        message={confirmTask === "dnsFlush"
          ? t("maintenance.dnsFlush.confirmBody")
          : t("maintenance.quicklookCache.confirmBody")}
        confirmLabel={t("maintenance.run")}
        cancelLabel={t("maintenance.cancelLabel")}
        variant="warning"
        onConfirm={() => void run()}
        onCancel={() => setConfirmTask(null)}
      />
    </section>
  );
}
