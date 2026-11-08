// useScanLifecycle — the scan state machine, extracted from App.tsx.
//
// Owns everything that turns raw scan runs into appStore state:
//   - restoring the latest completed scan at startup
//   - progress / finished event wiring
//   - the 1 s reconcile loop while a scan is running (events can be missed
//     while the webview mounts, or for directories that finish instantly)
//
// Pure state orchestration: no UI, no user actions (see useScanActions).

import { useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { appStore, useAppStore } from "../app/store";
import { useScanEvents } from "./useScanEvents";
import { errorMessage } from "../lib/errors";
import { getLatestScan, getScanSummary, listInsights, listLargeFiles } from "../lib/tauri";
import type { ScanFinished, ScanProgress as ScanProgressType } from "../types/scan";

export function useScanLifecycle() {
  const { t } = useTranslation();
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
}
