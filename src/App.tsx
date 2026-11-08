import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { AnalysisPanel } from "./components/analysis/AnalysisPanel";
import { FileManagerView } from "./components/fileManager";
import { CleanerCenter } from "./components/cleaner/CleanerCenter";
import { SessionsPanel } from "./components/sessions/SessionsPanel";
import { OverviewView } from "./components/overview/OverviewView";
import { OrganizeView } from "./components/organize/OrganizeView";
import { HistoryView } from "./components/history/HistoryView";
import { SettingsView } from "./components/settings/SettingsView";
import { Sidebar } from "./components/shell/Sidebar";
import { Topbar } from "./components/shell/Topbar";
import CommandPalette from "./components/shell/CommandPalette";
import { appStore, useAppStore } from "./app/store";
import { useScanActions } from "./hooks/useScanActions";
import { useScanLifecycle } from "./hooks/useScanLifecycle";
import { errorMessage } from "./lib/errors";
import { findDuplicates, listInsights, revealPath } from "./lib/tauri";
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
  const [storagePending, setStoragePending] = useState(false);
  const [fileOperationPending, setFileOperationPending] = useState(false);

  useScanLifecycle();

  const scanLocked = storagePending || fileOperationPending;
  const { scanAction, handleChoose, handleShortcut, handleStart, handleCancel } =
    useScanActions(scanLocked);

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

  const seedOrganizeAndNavigate = useCallback((sourceDir: string) => {
    appStore.seedOrganize(sourceDir);
    navigate("organize");
  }, [navigate]);

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

  const handleDiscoverySettings = useCallback(async (largeFileThreshold: number, staleDays: number) => {
    appStore.setDiscoverySettings(largeFileThreshold, staleDays);
    const summary = appStore.getState().summary;
    if (!summary) return;
    try {
      const insights = await listInsights(summary.scanId, largeFileThreshold, staleDays);
      appStore.setInsights(insights);
    } catch (error) {
      appStore.fail(errorMessage(error));
    }
  }, []);

  const handleReveal = useCallback((path: string) => {
    void revealPath(path).catch((error) => appStore.fail(errorMessage(error)));
  }, []);

  const statusText = fileOperationPending ? t("fileManager.state.working") : app.phase === "running"
    ? t("workspace.scanning") : currentResult ? t("workspace.scanned", { count: currentResult.totalFiles })
    : app.loading ? t("status.loading") : t("workspace.scanFirst");

  return (
    <main className="app-shell app-shell--workspace" data-han-scope data-theme="ink" data-luma-accent="celadon">
      <CommandPalette scanId={currentResult?.scanId ?? null} onNewScan={handleChoose}
        onNavigate={navigate} />
      <Sidebar activeView={effectiveView} viewsDisabled={viewsDisabled} onNavigate={navigate} />

      <div className="app-body">
        <Topbar
          mode={topbarModeFor(effectiveView)}
          view={effectiveView}
          selectedPath={app.selectedPath}
          includeHidden={app.includeHidden}
          isRunning={app.phase === "running"}
          loading={app.loading}
          hasResult={currentResult !== null}
          statusText={statusText}
          fileOperationPending={fileOperationPending}
          storagePending={storagePending}
          pendingAction={scanAction.pending}
          onChoose={handleChoose}
          onIncludeHiddenChange={appStore.setIncludeHidden}
          onStart={handleStart}
          onCancel={handleCancel}
          onIndexCleared={appStore.clearIndex}
          onIndexWorkingChange={setStoragePending}
        />

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
                storageLocked={app.loading || app.phase === "running" || scanLocked || scanAction.pending !== null} />
            </section>
          )}

          {/* Workspace panels stay mounted (hidden) once a scan result exists
              so search results, the file manager position and the organize
              flow survive switching views. */}
          {currentResult && <>
            <section id="workspace-panel-analysis" className="analysis-tab-panel" role="tabpanel"
              aria-labelledby="workspace-tab-analysis" hidden={effectiveView !== "analysis"}>
              <AnalysisPanel summary={currentResult}
                onDiscoverySettingsChange={handleDiscoverySettings} onReveal={handleReveal}
                onOrganize={seedOrganizeAndNavigate} onFindDuplicates={findAndGoDuplicates} />
            </section>
            <section id="workspace-panel-files" className="fm-tab-panel" role="tabpanel"
              aria-labelledby="workspace-tab-files" hidden={effectiveView !== "files"}>
              <FileManagerView scanId={currentResult.scanId} rootPath={currentResult.rootPath}
                onWorkingChange={setFileOperationPending} onOrganize={seedOrganizeAndNavigate} />
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
