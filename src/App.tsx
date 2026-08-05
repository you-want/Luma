import { useCallback, useEffect } from "react";
import { Database, ShieldCheck, TriangleAlert } from "lucide-react";
import { appStore, useAppStore } from "./app/store";
import { CategoryList } from "./components/CategoryList";
import { Duplicates } from "./components/Duplicates";
import { InsightList } from "./components/InsightList";
import { LargeFiles } from "./components/LargeFiles";
import Projects from "./components/Projects";
import { ScanControls } from "./components/ScanControls";
import { ScanHistory } from "./components/ScanHistory";
import { ScanProgress } from "./components/ScanProgress";
import { StorageOverview } from "./components/StorageOverview";
import { useScanEvents } from "./hooks/useScanEvents";
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

function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error && typeof error === "object" && "message" in error) {
    return String(error.message);
  }
  return "操作没有完成，请检查目录读取权限后重试。";
}

function App() {
  const app = useAppStore();

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
        appStore.fail(finished.error?.message || "扫描未完成，请重试。" );
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
        else if (current.status === "failed") appStore.fail("扫描未完成，请检查目录读取权限后重试。");
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
    <main className="app-shell">
      <header className="app-header">
        <img className="brand-mark" src="/luma-logo.svg" alt="" />
        <div className="brand-copy"><strong>Luma</strong><span>本地空间观察</span></div>
        <div className="privacy-note"><ShieldCheck size={15} />只读扫描，数据保留在本机</div>
      </header>

      <div className="content">
        <div className="intro">
          <h1>本地空间观察</h1>
          <p>选择一个目录，Luma 会在本机建立可恢复的空间索引，不修改任何原文件。</p>
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
            <div><strong>扫描遇到问题</strong><p>{app.error}</p></div>
          </section>
        )}

        {app.phase === "cancelled" && (
          <section className="status-message">
            <Database size={20} />
            <div><strong>扫描已取消</strong><p>未完成的结果不会覆盖最近一次成功扫描。</p></div>
          </section>
        )}

        {app.loading && <div className="loading-state">正在读取本地索引...</div>}

        {app.phase === "idle" && !app.loading && (
          <section className="empty-state">
            <Database size={28} />
            <h2>从一个目录开始</h2>
            <p>完成扫描后，这里会展示空间分类、最大文件和可解释的本地发现。</p>
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
            <Duplicates scanId={app.summary.scanId} />
            <Projects scanId={app.summary.scanId} />
            <ScanHistory scanId={app.summary.scanId} />
          </div>
        )}
      </div>
    </main>
  );
}

export default App;
