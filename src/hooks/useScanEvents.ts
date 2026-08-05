import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import type { ScanFinished, ScanProgress } from "../types/scan";

type ScanEventHandlers = {
  onProgress: (progress: ScanProgress) => void;
  onFinished: (finished: ScanFinished) => void;
};

export function useScanEvents({ onProgress, onFinished }: ScanEventHandlers) {
  useEffect(() => {
    const unlistenProgress = listen<ScanProgress>("scan-progress", (event) =>
      onProgress(event.payload),
    );
    const unlistenFinished = listen<ScanFinished>("scan-finished", (event) =>
      onFinished(event.payload),
    );

    return () => {
      void unlistenProgress.then((unlisten) => unlisten());
      void unlistenFinished.then((unlisten) => unlisten());
    };
  }, [onFinished, onProgress]);
}
