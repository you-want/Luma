// useScanActions — user-initiated scan flows, extracted from App.tsx.
//
// Covers choose-directory, quick locations (desktop/downloads), start and
// cancel. Every handler re-checks the store at call time and is debounced by
// useAsyncAction, so double clicks and cross-view locks stay safe.

import { useCallback } from "react";
import { desktopDir, downloadDir } from "@tauri-apps/api/path";
import { appStore } from "../app/store";
import { useAsyncAction } from "./useAsyncAction";
import { errorMessage } from "../lib/errors";
import { cancelScan, chooseDirectory, startScan } from "../lib/tauri";

export type ScanActionPending = "choose" | "desktop" | "downloads" | "start" | "cancel";

export function useScanActions(locked: boolean) {
  const scanAction = useAsyncAction<ScanActionPending>();

  const handleChoose = useCallback(async () => {
    if (locked || appStore.getState().phase === "running") return;
    await scanAction.run("choose", async () => {
      try {
        const path = await chooseDirectory();
        if (path) appStore.setSelectedPath(path);
      } catch (error) {
        appStore.fail(errorMessage(error));
      }
    });
  }, [locked, scanAction]);

  const handleShortcut = useCallback(async (location: "desktop" | "downloads") => {
    if (locked || appStore.getState().phase === "running") return;
    await scanAction.run(location, async () => {
      try {
        const path = await (location === "desktop" ? desktopDir() : downloadDir());
        appStore.setSelectedPath(path);
      } catch (error) {
        appStore.fail(errorMessage(error));
      }
    });
  }, [locked, scanAction]);

  const handleStart = useCallback(async () => {
    const state = appStore.getState();
    if (!state.selectedPath || locked || state.phase === "running") return;
    await scanAction.run("start", async () => {
      try {
        const scanId = await startScan({
          rootPath: state.selectedPath,
          includeHidden: state.includeHidden,
          stayOnFileSystem: true,
        });
        appStore.start(scanId);
      } catch (error) {
        appStore.fail(errorMessage(error));
      }
    });
  }, [locked, scanAction]);

  const handleCancel = useCallback(async () => {
    const scanId = appStore.getState().activeScanId;
    if (!scanId) return;
    await scanAction.run("cancel", async () => {
      try {
        await cancelScan(scanId);
      } catch (error) {
        appStore.fail(errorMessage(error));
      }
    });
  }, [scanAction]);

  return { scanAction, handleChoose, handleShortcut, handleStart, handleCancel };
}
