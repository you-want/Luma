import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { desktopDir, downloadDir } from "@tauri-apps/api/path";
import { Eye, EyeOff, FolderOpen, Play, RefreshCw, Square } from "lucide-react";
import { FileManagerView } from "./components/fileManager";
import { CleanerCenter } from "./components/cleaner/CleanerCenter";
import { SessionsPanel } from "./components/sessions/SessionsPanel";
import { OverviewView } from "./components/overview/OverviewView";
import { OrganizeView } from "./components/organize/OrganizeView";
import { HistoryView } from "./components/history/HistoryView";
import { SettingsView } from "./components/settings/SettingsView";
import { Sidebar } from "./components/Sidebar";
import { appStore, useAppStore } from "./app/store";
import { CategoryList } from "./components/CategoryList";
import { CleanupPanel } from "./components/CleanupPanel";
import CommandPalette from "./components/CommandPalette";
import { Duplicates } from "./components/Duplicates";
import { InsightList } from "./components/InsightList";
import { LanguageSwitcher } from "./components/LanguageSwitcher";
import { LargeFiles } from "./components/LargeFiles";
import Projects from "./components/Projects";
import { ScanHistory } from "./components/ScanHistory";
import Search from "./components/Search";
import { StorageOverview } from "./components/StorageOverview";
import { IndexStorage } from "./components/IndexStorage";
import { UpdateButton } from "./components/UpdateButton";
import { ButtonSpinner } from "./components/ButtonSpinner";
import { useScanEvents } from "./hooks/useScanEvents";
import { useAsyncAction } from "./hooks/useAsyncAction";
import { errorMessage } from "./lib/errors";
import { basename } from "./lib/format";
import {
  cancelScan,
  chooseDirectory,
  findDuplicates,
  getLatestScan,
  getScanSummary,
  listInsights,
  listLargeFiles,
  revealPath,
  startScan,
} from "./lib/tauri";
import type { ScanFinished, ScanProgress as ScanProgressType } from "./types/scan";
import type { AppView } from "./types/navigation";
import { ALL_VIEWS, isScanFreeView, topbarModeFor } from "./types/navigation";
import "./App.css";

const VIEW_STORAGE_KEY = "luma.view";

/** Views that need a scan result; the sidebar keeps them gated until then. */
function isScanGatedView(view: AppView): boolean {
  return !isScanFreeView(view);
}

function App() {
  const { t } = useTranslation();
  const app = useAppStore();
  const [activeView, setActiveView] = useState<AppView>(() => {
    const stored = localStorage.getItem(VIEW_STORAGE_KEY) as AppView | null;
    return stored && ALL_VIEWS.includes(stored) ? stored : "overview";
  });
  const scanAction = useAsyncAction<"choose" | "desktop" | "downloads" | "start" | "cancel">();
  const [storagePending, setStoragePending] = useState(false);
  const [fileOperationPending, setFileOperationPending] = useState(false);
  const currentResult = app.phase === "completed" && app.summary
    && app.selectedPath.replace(/[/\\]+$/, "") === app.summary.rootPath.replace(/[/\\]+$/, "")
    && scanAction.pending !== "start" ? app.summary : null;
  const viewsDisabled = !currentResult || scanAction.pending !== null;

  // A persisted gated view (analysis/files/organize) falls back to the
  // overview welcome state until a scan result is available again.
  const effectiveView: AppView =
    isScanGatedView(activeView) && !currentResult ? "overview" : activeView;

  const navigate = useCallback((view: AppView, anchor?: string) => {
    setActiveView(view);
    localStorage.setItem(VIEW_STORAGE_KEY, view);
    if (anchor) {
      // The target panel may just have been un-hidden; wait a tick so
      // scrollIntoView sees the laid-out element.
      window.setTimeout(() => {
        document.querySelector(anchor)?.scrollIntoView({ behavior: "smooth", block: "start" });
      }, 80);
    }
  }, []);

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
      .catch(error => appStore.fail(errorMessage(error)));
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
  }, [app.activeScanId, app.phase, loadResult, t]);

  async function handleChoose() {
    if (storagePending || fileOperationPending || appStore.getState().phase === "running") return;
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
    if (storagePending || fileOperationPending || appStore.getState().phase === "running") return;
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
    if (!app.selectedPath || storagePending || fileOperationPending || appStore.getState().phase === "running") return;
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

  // Cleanup hints → duplicates: kick off the content-hash search (if it has
  // not run yet) and land on the duplicates section.
  const findAndGoDuplicates = useCallback(() => {
    const state = appStore.getState();
    if (state.phase === "completed" && state.summary && state.duplicates.status === "idle") {
      appStore.setDuplicates({ status: "loading" });
      findDuplicates(state.summary.scanId)
        .then((groups) => appStore.setDuplicates({ status: "ready", groups }))
        .catch((error) => appStore.setDuplicates({
          status: "error",
          message: errorMessage(error, t("duplicates.findError")),
        }));
    }
    navigate("analysis", ".duplicates-section");
  }, [navigate, t]);

  const topbarMode = topbarModeFor(effectiveView);
  const scanControlsDisabled = app.loading || app.phase === "running" || storagePending
    || fileOperationPending || scanAction.pending !== null;
  const statusText = fileOperationPending ? t("fileManager.state.working") : app.phase === "running"
    ? t("workspace.scanning") : currentResult ? t("workspace.scanned", { count: currentResult.totalFiles })
    : app.loading ? t("status.loading") : t("workspace.scanFirst");
  const topbarTitle = effectiveView === "cleaner" ? t("cleaner.title")
    : effectiveView === "sessions" ? t("sessions.title")
    : effectiveView === "history" ? t("nav.views.history")
    : t("nav.views.settings");
  const topbarNote = effectiveView === "cleaner" ? t("topbar.machineCleaner")
    : effectiveView === "sessions" ? t("topbar.machineSessions")
    : effectiveView === "history" ? t("topbar.history")
    : t("topbar.settings");

  return (
    <main className="app-shell app-shell--workspace" data-han-scope data-theme="ink" data-luma-accent="celadon">
      <CommandPalette scanId={currentResult?.scanId ?? null} onNewScan={handleChoose}
        onNavigate={navigate} />
      <Sidebar activeView={effectiveView} viewsDisabled={viewsDisabled} onNavigate={navigate} />

      <div className="app-body">
        <header className="app-topbar">
          {topbarMode === "workspace" && <div className="topbar-workspace">
            <button type="button" className="topbar-dir-pill" onClick={handleChoose}
              disabled={scanControlsDisabled} aria-busy={scanAction.pending === "choose"}
              title={app.selectedPath || t("scanControls.startHint")}>
              {scanAction.pending === "choose" ? <ButtonSpinner size={15} /> : <FolderOpen size={15} aria-hidden="true" />}
              <span className="topbar-dir-name">{app.selectedPath
                ? basename(app.selectedPath) || app.selectedPath
                : t("scanControls.notSelected")}</span>
            </button>
            <p className="topbar-status" role="status">{statusText}</p>
            <span className="topbar-spacer" />
            <label className="toggle-control topbar-hidden-toggle" title={t("workspace.hiddenHint")}>
              <span aria-hidden="true">{app.includeHidden ? <Eye size={14} /> : <EyeOff size={14} />}</span>
              <span className="visually-hidden">{t("scanControls.includeHidden")}</span>
              <input type="checkbox" checked={app.includeHidden} disabled={scanControlsDisabled}
                onChange={event => appStore.setIncludeHidden(event.target.checked)} />
              <span className="switch" aria-hidden="true" />
            </label>
            {app.phase === "running" ? (
              <button className="button button-danger topbar-scan-button" type="button" onClick={handleCancel}
                disabled={scanAction.pending !== null} aria-busy={scanAction.pending === "cancel"}>
                {scanAction.pending === "cancel" ? <ButtonSpinner /> : <Square size={13} fill="currentColor" aria-hidden="true" />}
                {t(scanAction.pending === "cancel" ? "scanControls.cancelling" : "scanControls.cancel")}
              </button>
            ) : (
              <button className="button button-primary topbar-scan-button" type="button" onClick={handleStart}
                disabled={!app.selectedPath || scanControlsDisabled} aria-busy={scanAction.pending === "start"}>
                {scanAction.pending === "start" ? <ButtonSpinner size={14} />
                  : currentResult ? <RefreshCw size={14} aria-hidden="true" />
                  : <Play size={14} fill="currentColor" aria-hidden="true" />}
                {t(scanAction.pending === "start" ? "scanControls.starting" : currentResult ? "workspace.rescan" : "scanControls.scan")}
              </button>
            )}
          </div>}
          {topbarMode === "machine" && <div className="topbar-static">
            <strong className="topbar-title">{topbarTitle}</strong>
            <span className="topbar-note">{topbarNote}</span>
          </div>}
          {topbarMode === "plain" && <div className="topbar-static">
            <strong className="topbar-title">{topbarTitle}</strong>
            <span className="topbar-note">{topbarNote}</span>
          </div>}

          <div className="topbar-global">
            <IndexStorage disabled={app.loading || app.phase === "running" || fileOperationPending || scanAction.pending !== null}
              onCleared={appStore.clearIndex} onWorkingChange={setStoragePending} />
            <UpdateButton />
            <LanguageSwitcher />
          </div>
        </header>

        <div className="workspace-content">
          {effectiveView === "overview" && (
            <section id="workspace-panel-overview" className="analysis-tab-panel" role="tabpanel"
              aria-labelledby="workspace-tab-overview">
              <OverviewView
                result={currentResult}
                phase={app.phase}
                progress={app.progress}
                loading={app.loading}
                hasSelectedPath={Boolean(app.selectedPath)}
                pendingAction={scanAction.pending}
                onChoose={handleChoose}
                onShortcut={handleShortcut}
                onStart={handleStart}
                onNavigate={navigate}
              />
            </section>
          )}

          {effectiveView === "cleaner" && (
            <section id="workspace-panel-cleaner" className="analysis-tab-panel" role="tabpanel"
              aria-labelledby="workspace-tab-cleaner">
              <div className="content">
                <div className="results">
                  <CleanerCenter />
                </div>
              </div>
            </section>
          )}

          {effectiveView === "sessions" && (
            <section id="workspace-panel-sessions" className="analysis-tab-panel" role="tabpanel"
              aria-labelledby="workspace-tab-sessions">
              <div className="content">
                <div className="results">
                  <SessionsPanel />
                </div>
              </div>
            </section>
          )}

          {effectiveView === "history" && (
            <section id="workspace-panel-history" className="analysis-tab-panel" role="tabpanel"
              aria-labelledby="workspace-tab-history">
              <HistoryView currentScanId={currentResult?.scanId ?? null}
                currentRootPath={currentResult?.rootPath ?? null} />
            </section>
          )}

          {effectiveView === "settings" && (
            <section id="workspace-panel-settings" className="analysis-tab-panel" role="tabpanel"
              aria-labelledby="workspace-tab-settings">
              <SettingsView onNavigate={navigate} onDiscoverySettingsChange={handleDiscoverySettings}
                onIndexCleared={appStore.clearIndex} onWorkingChange={setStoragePending}
                storageLocked={app.loading || app.phase === "running" || fileOperationPending || scanAction.pending !== null} />
            </section>
          )}

          {/* Workspace panels stay mounted (hidden) once a scan result exists
              so search results, the file manager position and the organize
              flow survive switching views. */}
          {currentResult && <>
            <section id="workspace-panel-analysis" className="analysis-tab-panel" role="tabpanel"
              aria-labelledby="workspace-tab-analysis" hidden={effectiveView !== "analysis"}>
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
                  <LargeFiles files={app.largeFiles} onReveal={handleReveal}
                    scanId={currentResult.scanId} />
                  <CleanupPanel scanId={currentResult.scanId} rootPath={currentResult.rootPath}
                    onOrganize={(sourceDir) => { appStore.seedOrganize(sourceDir); navigate("organize"); }}
                    onFindDuplicates={findAndGoDuplicates} />
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
              aria-labelledby="workspace-tab-files" hidden={effectiveView !== "files"}>
              <FileManagerView scanId={currentResult.scanId} rootPath={currentResult.rootPath}
                onWorkingChange={setFileOperationPending}
                onOrganize={(sourceDir) => { appStore.seedOrganize(sourceDir); navigate("organize"); }} />
            </section>
            <OrganizeView scanId={currentResult.scanId} rootPath={currentResult.rootPath}
              hidden={effectiveView !== "organize"} onNavigate={navigate} />
          </>}
        </div>
      </div>
    </main>
  );
}

export default App;
