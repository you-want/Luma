import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { desktopDir, downloadDir } from "@tauri-apps/api/path";
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
import { useAsyncAction } from "./hooks/useAsyncAction";
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
  const scanAction = useAsyncAction<"choose" | "desktop" | "downloads" | "start" | "cancel">();
  const [fileOperationPending, setFileOperationPending] = useState(false);
  const currentResult = app.phase === "completed" && app.summary
    && app.selectedPath.replace(/[/\\]+$/, "") === app.summary.rootPath.replace(/[/\\]+$/, "")
    && scanAction.pending !== "start" ? app.summary : null;
  const viewsDisabled = !currentResult || scanAction.pending !== null;

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
    if (fileOperationPending || appStore.getState().phase === "running") return;
    await scanAction.run("choose", async () => {
      try {
        const path = await chooseDirectory();
        if (path) appStore.setSelectedPath(path);
      } catch (error) {
        appStore.fail(errorMessage(error));
      }
    });
  }

  async function handleShortcut(location: "desktop" | "downloads") {
    if (fileOperationPending || appStore.getState().phase === "running") return;
    await scanAction.run(location, async () => {
      try {
        const path = await (location === "desktop" ? desktopDir() : downloadDir());
        appStore.setSelectedPath(path);
      } catch (error) {
        appStore.fail(errorMessage(error));
      }
    });
  }

  async function handleStart() {
    if (!app.selectedPath || fileOperationPending || appStore.getState().phase === "running") return;
    await scanAction.run("start", async () => {
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
    });
  }

  async function handleCancel() {
    const scanId = app.activeScanId;
    if (!scanId) return;
    await scanAction.run("cancel", async () => {
      try {
        await cancelScan(scanId);
      } catch (error) {
        appStore.fail(errorMessage(error));
      }
    });
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
    <main className="app-shell app-shell--workspace" data-han-scope data-theme="ink" data-luma-accent="celadon">
      <CommandPalette scanId={currentResult?.scanId ?? null} onNewScan={handleChoose} />
      <header className="app-header">
        <img className="brand-mark" src="/luma-logo.svg" alt="" />
        <div className="brand-copy"><strong>{t("app.name")}</strong><span>{t("app.tagline")}</span></div>
        <div className="privacy-note"><ShieldCheck size={15} />{t("app.privacyNote")}</div>
        <UpdateButton />
        <LanguageSwitcher />
      </header>

      <ScanControls
        selectedPath={app.selectedPath}
        includeHidden={app.includeHidden}
        isRunning={app.phase === "running"}
        isLoading={app.loading}
        hasResult={currentResult !== null}
        locked={fileOperationPending}
        pendingAction={scanAction.pending}
        statusText={fileOperationPending ? t("fileManager.state.working") : app.phase === "running"
          ? t("workspace.scanning") : currentResult ? t("workspace.scanned", { count: currentResult.totalFiles })
          : app.loading ? t("status.loading") : t("workspace.scanFirst")}
        onChoose={handleChoose}
        onShortcut={handleShortcut}
        onStart={handleStart}
        onCancel={handleCancel}
        onIncludeHiddenChange={appStore.setIncludeHidden}
      >
        <nav className="app-tab-bar" role="tablist" aria-label={t("workspace.views")}
          onKeyDown={event => {
            if (viewsDisabled || !["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
            event.preventDefault();
            const next = event.key === "Home" ? "analysis" : event.key === "End" ? "files"
              : activeTab === "analysis" ? "files" : "analysis";
            setActiveTab(next);
            document.getElementById(`workspace-tab-${next}`)?.focus();
          }}>
          {(["analysis", "files"] as const).map(tab => (
            <button key={tab} id={`workspace-tab-${tab}`} type="button" role="tab"
              aria-selected={activeTab === tab} aria-controls={`workspace-panel-${tab}`}
              tabIndex={activeTab === tab ? 0 : -1} disabled={viewsDisabled}
              className={`app-tab${activeTab === tab ? " app-tab--active" : ""}`}
              title={viewsDisabled ? t("workspace.scanFirst") : undefined}
              onClick={() => setActiveTab(tab)}>
              {tab === "analysis" ? <BarChart2 size={15} /> : <FolderOpen size={15} />}
              {t(tab === "analysis" ? "workspace.analysis" : "workspace.files")}
            </button>
          ))}
        </nav>
      </ScanControls>

      <div className="workspace-content">
        {!currentResult && <div className="workspace-status-panel"><div className="content">
          {app.phase === "running" && <ScanProgress progress={app.progress} />}
          {app.phase === "failed" && <section className="status-message status-error" role="alert">
            <TriangleAlert size={20} />
            <div><strong>{t("status.failedTitle")}</strong><p>{app.error}</p></div>
          </section>}
          {app.phase === "cancelled" && <section className="status-message">
            <Database size={20} />
            <div><strong>{t("status.cancelledTitle")}</strong><p>{t("status.cancelledBody")}</p></div>
          </section>}
          {app.loading && <div className="loading-state">{t("status.loading")}</div>}
          {!app.loading && app.phase !== "running" && <section className="workspace-welcome">
            <FolderOpen size={32} aria-hidden="true" />
            <h1>{t(app.selectedPath ? "workspace.readyTitle" : "workspace.welcomeTitle")}</h1>
            <p>{t(app.selectedPath ? "workspace.readyBody" : "workspace.welcomeBody")}</p>
            <ol className="workspace-flow" aria-label={t("workspace.flowLabel")}>
              <li><span>1</span>{t("workspace.choose")}</li>
              <li><span>2</span>{t("scanControls.scan")}</li>
              <li><span>3</span>{t("workspace.useViews")}</li>
            </ol>
          </section>}
        </div></div>}

        {currentResult && <>
          <section id="workspace-panel-analysis" className="analysis-tab-panel" role="tabpanel"
            aria-labelledby="workspace-tab-analysis" hidden={activeTab !== "analysis"}>
            <div className="content">
            <div className="results">
              <StorageOverview summary={currentResult} />
              <div className="results-grid">
                <CategoryList categories={currentResult.categories} totalBytes={currentResult.totalBytes} />
                <InsightList
                  scanId={currentResult.scanId}
                  insights={app.insights}
                  largeFileThreshold={app.largeFileThreshold}
                  staleDays={app.staleDays}
                  onSettingsChange={handleDiscoverySettings}
                />
              </div>
              <LargeFiles files={app.largeFiles} onReveal={handleReveal} />
              <CleanupPanel scanId={currentResult.scanId} />
              <Search
                scanId={currentResult.scanId}
                categories={currentResult.categories.map((c) => c.category)}
              />
              <Duplicates scanId={currentResult.scanId} />
              <Projects scanId={currentResult.scanId} />
              <ScanHistory scanId={currentResult.scanId} />
            </div>
            </div>
          </section>
          <section id="workspace-panel-files" className="fm-tab-panel" role="tabpanel"
            aria-labelledby="workspace-tab-files" hidden={activeTab !== "files"}>
            <FileManagerView key={currentResult.scanId} scanId={currentResult.scanId} rootPath={currentResult.rootPath}
              onWorkingChange={setFileOperationPending} />
          </section>
        </>}
      </div>
    </main>
  );
}

export default App;
