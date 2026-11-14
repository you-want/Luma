// SettingsView — one place for the preferences that used to live in four
// different spots (scan controls, insight filters, the cleaner card and the
// header). Every control here is wired to the same store/backend commands the
// original controls used, so there is exactly one source of truth.

import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { useTranslation } from "react-i18next";
import { ChevronRight, ShieldCheck } from "lucide-react";
import { getCleanerWhitelist, getTrayMonitorEnabled, setTrayMonitorEnabled } from "../../lib/tauri";
import { appStore, useAppStore } from "../../app/store";
import { LanguageSwitcher } from "../shell/LanguageSwitcher";
import { IndexStorage } from "../shell/IndexStorage";
import { UpdateButton } from "../shell/UpdateButton";
import type { AppView } from "../../types/navigation";
import type { CleanerWhitelistEntry } from "../../types/scan";
import "./Settings.css";

type Props = {
  onNavigate: (view: AppView, anchor?: string) => void;
  onDiscoverySettingsChange: (largeFileThreshold: number, staleDays: number) => void;
  onIndexCleared: () => void;
  onWorkingChange: (working: boolean) => void;
  storageLocked: boolean;
};

export function SettingsView({
  onNavigate,
  onDiscoverySettingsChange,
  onIndexCleared,
  onWorkingChange,
  storageLocked,
}: Props) {
  const { t } = useTranslation();
  const app = useAppStore();
  const [trayEnabled, setTrayEnabled] = useState<boolean | null>(null);
  const [whitelist, setWhitelist] = useState<CleanerWhitelistEntry[] | null>(null);
  const [version, setVersion] = useState<string | null>(null);

  // Tray monitor switch — kept in sync with the tray menu and the cleaner
  // card through the same backend event (CLN-403).
  useEffect(() => {
    getTrayMonitorEnabled().then(setTrayEnabled).catch(() => setTrayEnabled(null));
    const unlisten = listen<boolean>("tray-monitor-changed", (event) => {
      setTrayEnabled(event.payload);
    });
    return () => { void unlisten.then((off) => off()); };
  }, []);

  useEffect(() => {
    getCleanerWhitelist().then(setWhitelist).catch(() => setWhitelist(null));
  }, []);

  useEffect(() => {
    getVersion().then(setVersion).catch(() => setVersion(null));
  }, []);

  async function toggleTrayMonitor() {
    if (trayEnabled === null) return;
    const next = !trayEnabled;
    setTrayEnabled(next); // optimistic; the backend event reconciles
    try {
      await setTrayMonitorEnabled(next);
    } catch {
      setTrayEnabled(!next);
    }
  }

  return (
    <div className="workspace-status-panel settings-panel"><div className="content">
      <div className="settings-grid">
        <section className="settings-card" aria-labelledby="settings-general">
          <h2 id="settings-general">{t("settings.general.title")}</h2>
          <div className="settings-row">
            <div className="settings-copy">
              <strong>{t("settings.general.language")}</strong>
              <p>{t("settings.general.languageHint")}</p>
            </div>
            <LanguageSwitcher />
          </div>
          <div className="settings-row">
            <div className="settings-copy">
              <strong>{t("settings.general.includeHidden")}</strong>
              <p>{t("settings.general.includeHiddenHint")}</p>
            </div>
            <label className="toggle-control">
              <span className="visually-hidden">{t("settings.general.includeHidden")}</span>
              <input type="checkbox" checked={app.includeHidden}
                onChange={(event) => appStore.setIncludeHidden(event.target.checked)} />
              <span className="switch" aria-hidden="true" />
            </label>
          </div>
        </section>

        <section className="settings-card" aria-labelledby="settings-scan">
          <h2 id="settings-scan">{t("settings.scan.title")}</h2>
          <div className="settings-row">
            <div className="settings-copy">
              <strong>{t("settings.scan.largeFileThreshold")}</strong>
              <p>{t("settings.scan.largeFileThresholdHint")}</p>
            </div>
            <select value={app.largeFileThreshold} onChange={(event) =>
              onDiscoverySettingsChange(Number(event.target.value), app.staleDays)}>
              <option value={256 * 1024 ** 2}>{t("insight.over256mb")}</option>
              <option value={1024 ** 3}>{t("insight.over1gb")}</option>
              <option value={5 * 1024 ** 3}>{t("insight.over5gb")}</option>
            </select>
          </div>
          <div className="settings-row">
            <div className="settings-copy">
              <strong>{t("settings.scan.staleDays")}</strong>
              <p>{t("settings.scan.staleDaysHint")}</p>
            </div>
            <select value={app.staleDays} onChange={(event) =>
              onDiscoverySettingsChange(app.largeFileThreshold, Number(event.target.value))}>
              <option value={90}>{t("insight.over90d")}</option>
              <option value={180}>{t("insight.over180d")}</option>
              <option value={365}>{t("insight.over1y")}</option>
            </select>
          </div>
          <div className="settings-row">
            <div className="settings-copy">
              <strong>{t("settings.scan.indexStorage")}</strong>
              <p>{t("settings.scan.indexStorageHint")}</p>
            </div>
            <IndexStorage disabled={storageLocked}
              onCleared={onIndexCleared} onWorkingChange={onWorkingChange} />
          </div>
        </section>

        <section className="settings-card" aria-labelledby="settings-tray">
          <h2 id="settings-tray">{t("settings.tray.title")}</h2>
          <div className="settings-row">
            <div className="settings-copy">
              <strong>{t("statusCard.trayMonitor")}</strong>
              <p>{t("settings.tray.hint")}</p>
            </div>
            {trayEnabled !== null && (
              <label className="toggle-control">
                <span className="visually-hidden">{t("statusCard.trayMonitor")}</span>
                <input type="checkbox" checked={trayEnabled} disabled={trayEnabled === null}
                  onChange={() => void toggleTrayMonitor()} />
                <span className="switch" aria-hidden="true" />
              </label>
            )}
          </div>
        </section>

        <section className="settings-card" aria-labelledby="settings-paths">
          <h2 id="settings-paths">{t("settings.paths.title")}</h2>
          <p className="settings-note"><ShieldCheck size={14} aria-hidden="true" />{t("cleaner.whitelist.note")}</p>
          {whitelist !== null && (
            whitelist.length === 0
              ? <p className="empty-inline">{t("cleaner.whitelist.empty")}</p>
              : <ul className="settings-path-list">
                {whitelist.slice(0, 4).map((entry) => (
                  <li key={entry.path} title={entry.path}>{entry.path}</li>
                ))}
                {whitelist.length > 4 && <li className="settings-path-more">
                  {t("settings.paths.more", { count: whitelist.length - 4 })}
                </li>}
              </ul>
          )}
          <button type="button" className="button settings-manage-btn" onClick={() => onNavigate("cleaner")}>
            {t("settings.paths.manage")}
            <ChevronRight size={14} aria-hidden="true" />
          </button>
        </section>

        <section className="settings-card settings-about" aria-labelledby="settings-about">
          <h2 id="settings-about">{t("settings.about.title")}</h2>
          <div className="settings-row">
            <div className="settings-copy">
              <strong>Luma{version ? ` · v${version}` : ""}</strong>
              <p>{t("settings.about.hint")}</p>
            </div>
            <UpdateButton />
          </div>
        </section>
      </div>
    </div></div>
  );
}
