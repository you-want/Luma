import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { BarChart2, Database, FolderOpen, ShieldCheck, TriangleAlert } from "lucide-react";
import { FileManagerView } from "./components/fileManager";
import { appStore, useAppStore } from "./app/store";
import { CategoryList } from "./components/CategoryList";
import { CleanupPanel } from "./components/CleanupPanel";
import CommandPalette from "./components/CommandPalette";
import { Duplicates } from "./components/Duplicates";
import { InsightList } from "./components/InsightList";
import { LanguageSwitcher } from "./components/LanguageSwitcher";
import { LargeFiles } from "./components/LargeFiles";
import Projects from "./components/Projects";
import { ScanControls } from "./components/ScanControls";
import { ScanHistory } from "./components/ScanHistory";
import { ScanProgress } from "./components/ScanProgress";
import Search from "./components/Search";
import { StorageOverview } from "./components/StorageOverview";
import { UpdateButton } from "./components/UpdateButton";
import { useScanEvents } from "./hooks/useScanEvents";
import { errorMessage } from "./lib/errors";
import {
  cancelScan,
  chooseDirectory,
  getLatestScan,
  getScanSummary,
  listInsights,
  listLargeFiles,
  revealPath,
  startScan,
} from "./lib/tauri";
import type { ScanFinished, ScanProgress as ScanProgressType } from "./types/scan";
import "./App.css";

type AppTab = "analysis" | "files";

function App() {
  const { t } = useTranslation();
  const app = useAppStore();
  const [activeTab, setActiveTab] = useState<AppTab>("analysis");

  const loadResult = useCallback(async (scanId: string) => {
    const settings = appStore.getState();
    const [summary, files, insights] = await Promise.all([
      getScanSummary(scanId),
      listLargeFiles(scanId),
      listInsights(scanId, settings.largeFileThreshold, settings.staleDays),
    ]);
    if (summary.status === "completed") appStore.complete(summary, files, insights);
  }, []);

  const handleProgress = useCallback((progress: ScanProgressType) => {
    appStore.setProgress(progress);
  }, []);

  const handleFinished = useCallback(
    async (finished: ScanFinished) => {
      if (finished.scanId !== appStore.getState().activeScanId) return;
      if (finished.status === "completed" && finished.summary) {
        try {
          await loadResult(finished.scanId);
        } catch (error) {
          appStore.fail(errorMessage(error));
        }
      } else if (finished.status === "cancelled") {
        appStore.cancel();
      } else {
        appStore.fail(errorMessage(finished.error));
      }
    },
    [loadResult],
  );

  useScanEvents({ onProgress: handleProgress, onFinished: handleFinished });

  useEffect(() => {
    getLatestScan()
      .then(async (summary) => {
        if (summary) await loadResult(summary.scanId);
        else appStore.ready();
      })
      .catch(() => appStore.ready());
  }, [loadResult]);

  // Events are the fast path, but a terminal event can be missed while the
  // webview is mounting or when a very small directory finishes immediately.
  // Reconcile the persisted run while scanning so the UI cannot remain stuck.
  useEffect(() => {
    if (app.phase !== "running" || !app.activeScanId) return;

    const scanId = app.activeScanId;
    let disposed = false;
    let checking = false;
    const reconcile = async () => {
      if (checking) return;
      checking = true;
      try {
        const current = await getScanSummary(scanId);
        if (disposed || appStore.getState().activeScanId !== scanId) return;
        if (current.status === "completed") await loadResult(scanId);
        else if (current.status === "cancelled") appStore.cancel();
        else if (current.status === "failed") appStore.fail(t("errors.SCAN_FAILED"));
      } catch (error) {
        if (!disposed && appStore.getState().activeScanId === scanId) {
          appStore.fail(errorMessage(error));
        }
      } finally {
        checking = false;
      }
    };

    void reconcile();
    const timer = window.setInterval(() => void reconcile(), 1000);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  }, [app.activeScanId, app.phase, loadResult]);

  async function handleChoose() {
    try {
      const path = await chooseDirectory();
      if (path) appStore.setSelectedPath(path);
    } catch (error) {
      appStore.fail(errorMessage(error));
    }
  }

  async function handleStart() {
    if (!app.selectedPath) return;
    try {
      const scanId = await startScan({
        rootPath: app.selectedPath,
        includeHidden: app.includeHidden,
        stayOnFileSystem: true,
      });
      appStore.start(scanId);
    } catch (error) {
      appStore.fail(errorMessage(error));
    }
  }

  async function handleCancel() {
    if (!app.activeScanId) return;
    try {
      await cancelScan(app.activeScanId);
    } catch (error) {
      appStore.fail(errorMessage(error));
    }
  }

  async function handleDiscoverySettings(largeFileThreshold: number, staleDays: number) {
    appStore.setDiscoverySettings(largeFileThreshold, staleDays);
    if (!app.summary) return;
    try {
      const insights = await listInsights(app.summary.scanId, largeFileThreshold, staleDays);
      appStore.setInsights(insights);
    } catch (error) {
      appStore.fail(errorMessage(error));
    }
  }

  function handleReveal(path: string) {
    void revealPath(path).catch((error) => appStore.fail(errorMessage(error)));
  }

  return (
    <main className="app-shell" data-han-scope data-theme="ink" data-luma-accent="celadon">
      <CommandPalette scanId={app.summary?.scanId ?? null} onNewScan={handleChoose} />
      <header className="app-header">
        <img className="brand-mark" src="/luma-logo.svg" alt="" />
        <div className="brand-copy"><strong>{t("app.name")}</strong><span>{t("app.tagline")}</span></div>
        {/* Tab bar — only shown after a scan completes */}
        {app.summary && app.phase === "completed" && (
          <nav className="app-tab-bar" role="tablist" aria-label="视图切换">
            <button
              type="button"
              role="tab"
              aria-selected={activeTab === "analysis"}
              className={`app-tab${activeTab === "analysis" ? " app-tab--active" : ""}`}
              onClick={() => setActiveTab("analysis")}
            >
              <BarChart2 size={13} />
              分析
            </button>
            <button
              type="button"
              role="tab"
              aria-selected={activeTab === "files"}
              className={`app-tab${activeTab === "files" ? " app-tab--active" : ""}`}
              onClick={() => setActiveTab("files")}
            >
              <FolderOpen size={13} />
              文件管理
            </button>
          </nav>
        )}
        <div className="privacy-note"><ShieldCheck size={15} />{t("app.privacyNote")}</div>
        <UpdateButton />
        <LanguageSwitcher />
      </header>

      {/* File manager tab — full-height, no content wrapper */}
      {app.summary && app.phase === "completed" && activeTab === "files" && (
        <div className="fm-tab-panel">
          <FileManagerView
            scanId={app.summary.scanId}
            rootPath={app.summary.rootPath}
          />
        </div>
      )}

      {/* Analysis tab (original scroll layout) */}
      {activeTab === "analysis" && (
        <div className="content">
          <div className="intro">
            <h1>{t("intro.title")}</h1>
            <p>{t("intro.description")}</p>
          </div>

          <ScanControls
            selectedPath={app.selectedPath}
            includeHidden={app.includeHidden}
            isRunning={app.phase === "running"}
            onChoose={handleChoose}
            onStart={handleStart}
            onCancel={handleCancel}
            onIncludeHiddenChange={appStore.setIncludeHidden}
          />

          {app.phase === "running" && <ScanProgress progress={app.progress} />}

          {app.phase === "failed" && (
            <section className="status-message status-error" role="alert">
              <TriangleAlert size={20} />
              <div><strong>{t("status.failedTitle")}</strong><p>{app.error}</p></div>
            </section>
          )}

          {app.phase === "cancelled" && (
            <section className="status-message">
              <Database size={20} />
              <div><strong>{t("status.cancelledTitle")}</strong><p>{t("status.cancelledBody")}</p></div>
            </section>
          )}

          {app.loading && <div className="loading-state">{t("status.loading")}</div>}

          {app.phase === "idle" && !app.loading && (
            <section className="empty-state">
              <Database size={28} />
              <h2>{t("empty.title")}</h2>
              <p>{t("empty.body")}</p>
            </section>
          )}

          {app.summary && app.phase === "completed" && (
            <div className="results">
              <StorageOverview summary={app.summary} />
              <div className="results-grid">
                <CategoryList categories={app.summary.categories} totalBytes={app.summary.totalBytes} />
                <InsightList
                  scanId={app.summary.scanId}
                  insights={app.insights}
                  largeFileThreshold={app.largeFileThreshold}
                  staleDays={app.staleDays}
                  onSettingsChange={handleDiscoverySettings}
                />
              </div>
              <LargeFiles files={app.largeFiles} onReveal={handleReveal} />
              <CleanupPanel scanId={app.summary.scanId} />
              <Search
                scanId={app.summary.scanId}
                categories={app.summary.categories.map((c) => c.category)}
              />
              <Duplicates scanId={app.summary.scanId} />
              <Projects scanId={app.summary.scanId} />
              <ScanHistory scanId={app.summary.scanId} />
            </div>
          )}
        </div>
      )}
    </main>
  );
}

export default App;
