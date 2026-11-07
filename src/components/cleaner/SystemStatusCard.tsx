// 系统状态 (System Status): compact CPU / memory / battery card. One
// snapshot per refresh (no background watch); refreshes on mount and on an
// 8-second interval.

import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Activity, Battery, BatteryCharging, Cpu, MemoryStick, RefreshCw } from "lucide-react";
import { getSystemStatus, getTrayMonitorEnabled, setTrayMonitorEnabled } from "../../lib/tauri";
import { formatBytes, formatNumber } from "../../lib/format";
import { errorMessage } from "../../lib/errors";
import type { SystemStatus } from "../../types/scan";

const REFRESH_INTERVAL_MS = 8000;

function UsageBar({ percent }: { percent: number }) {
  const clamped = Math.min(100, Math.max(0, Math.round(percent)));
  const danger = clamped >= 90;
  const warning = !danger && clamped >= 75;
  return (
    <div className="cleaner-volume-track" role="meter"
      aria-valuemin={0} aria-valuemax={100} aria-valuenow={clamped}>
      <div className={`cleaner-volume-fill${danger ? " is-danger" : warning ? " is-warning" : ""}`}
        style={{ width: `${clamped}%` }} />
    </div>
  );
}

export function SystemStatusCard() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<SystemStatus | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [trayEnabled, setTrayEnabled] = useState<boolean | null>(null);

  const refresh = useCallback(async () => {
    try {
      setStatus(await getSystemStatus());
      setLoadError(null);
    } catch (error) {
      setLoadError(errorMessage(error, t("statusCard.loadError")));
    }
  }, [t]);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), REFRESH_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, [refresh]);

  // Keep the in-app switch in sync with the tray menu toggle (CLN-403).
  useEffect(() => {
    getTrayMonitorEnabled()
      .then(setTrayEnabled)
      .catch(() => setTrayEnabled(null));
    const unlisten = listen<boolean>("tray-monitor-changed", (event) => {
      setTrayEnabled(event.payload);
    });
    return () => {
      void unlisten.then((off) => off());
    };
  }, []);

  const toggleTrayMonitor = useCallback(async () => {
    if (trayEnabled === null) return;
    const next = !trayEnabled;
    setTrayEnabled(next); // optimistic; the backend event reconciles
    try {
      await setTrayMonitorEnabled(next);
    } catch {
      setTrayEnabled(!next);
    }
  }, [trayEnabled]);

  return (
    <div className="result-section status-card" aria-labelledby="status-title">
      <div className="section-heading compact-heading">
        <h2 id="status-title"><Activity size={15} aria-hidden="true" /> {t("statusCard.title")}</h2>
        <button type="button" className="icon-button" title={t("statusCard.refresh")}
          aria-label={t("statusCard.refresh")} onClick={() => void refresh()}>
          <RefreshCw size={14} />
        </button>
      </div>

      {loadError && <p className="empty-inline cleaner-error" role="alert">{loadError}</p>}
      {status && !status.platformSupported && (
        <p className="empty-inline">{t("statusCard.unsupported")}</p>
      )}
      {trayEnabled !== null && (
        <label className="status-card-toggle">
          <input type="checkbox" checked={trayEnabled} onChange={() => void toggleTrayMonitor()} />
          {t("statusCard.trayMonitor")}
        </label>
      )}

      {status && status.platformSupported && (
        <div className="status-card-grid">
          <div className="status-card-row">
            <div className="status-card-label">
              <Cpu size={14} aria-hidden="true" />
              <span>{t("statusCard.cpu")}</span>
            </div>
            {status.cpuUsagePercent !== null ? (
              <>
                <UsageBar percent={status.cpuUsagePercent} />
                <span className="status-card-value">
                  {formatNumber(Math.round(status.cpuUsagePercent))}%
                </span>
              </>
            ) : (
              <span className="status-card-value text-secondary">{t("common.missing")}</span>
            )}
          </div>

          <div className="status-card-row">
            <div className="status-card-label">
              <MemoryStick size={14} aria-hidden="true" />
              <span>{t("statusCard.memory")}</span>
            </div>
            {status.memoryTotalBytes !== null && status.memoryUsedBytes !== null ? (
              <>
                <UsageBar percent={(status.memoryUsedBytes / status.memoryTotalBytes) * 100} />
                <span className="status-card-value">
                  {formatBytes(status.memoryUsedBytes)} / {formatBytes(status.memoryTotalBytes)}
                </span>
              </>
            ) : (
              <span className="status-card-value text-secondary">{t("common.missing")}</span>
            )}
          </div>

          {status.battery && (
            <div className="status-card-row">
              <div className="status-card-label">
                {status.battery.charging
                  ? <BatteryCharging size={14} aria-hidden="true" />
                  : <Battery size={14} aria-hidden="true" />}
                <span>{t("statusCard.battery")}</span>
              </div>
              <span className="status-card-value">
                {formatNumber(status.battery.percent)}% ·{" "}
                {status.battery.charging
                  ? t("statusCard.charging")
                  : t("statusCard.onBattery")}
              </span>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
