import { useCallback, useEffect, useRef, useState } from "react";
import { Download, RefreshCw } from "lucide-react";
import { getVersion } from "@tauri-apps/api/app";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { useTranslation } from "react-i18next";

type UpdateState = "idle" | "checking" | "available" | "downloading" | "current" | "error";

export function UpdateButton() {
  const { t } = useTranslation();
  const [state, setState] = useState<UpdateState>("checking");
  const [update, setUpdate] = useState<Update | null>(null);
  const [progress, setProgress] = useState(0);
  const [appVersion, setAppVersion] = useState<string>("");
  const downloadStats = useRef({ downloaded: 0, total: 0 });
  const checked = useRef(false);

  useEffect(() => {
    getVersion().then(setAppVersion).catch(() => {});
  }, []);

  const checkForUpdate = useCallback(async (silent = false) => {
    setState("checking");
    setProgress(0);
    try {
      const next = await check();
      setUpdate(next);
      if (next) {
        setState("available");
      } else if (silent) {
        setState("idle");
      } else {
        setState("current");
        window.setTimeout(() => setState("idle"), 2500);
      }
    } catch {
      setUpdate(null);
      setState(silent ? "idle" : "error");
    }
  }, []);

  useEffect(() => {
    if (checked.current) return;
    checked.current = true;
    void checkForUpdate(true);
  }, [checkForUpdate]);

  const handleUpdate = async () => {
    if (!update) {
      await checkForUpdate(false);
      return;
    }

    setState("downloading");
    downloadStats.current = { downloaded: 0, total: 0 };
    try {
      await update.downloadAndInstall((event: DownloadEvent) => {
        if (event.event === "Started") {
          downloadStats.current.total = event.data.contentLength ?? 0;
          setProgress(0);
        } else if (event.event === "Progress") {
          downloadStats.current.downloaded += event.data.chunkLength;
          const { downloaded, total } = downloadStats.current;
          if (total > 0) setProgress(Math.min(100, Math.round((downloaded / total) * 100)));
        } else {
          setProgress(100);
        }
      });
      await relaunch();
    } catch {
      setState("error");
    }
  };

  const label = (() => {
    switch (state) {
      case "checking":
        return t("update.checking");
      case "available":
        return t("update.available", { version: update?.version ?? "" });
      case "downloading":
        return t("update.downloading", { progress });
      case "current":
        return t("update.current");
      case "error":
        return t("update.error");
      default:
        return t("update.check");
    }
  })();

  const disabled = state === "checking" || state === "downloading" || state === "current";
  const title = update?.body && state === "available" ? `${label}\n\n${update.body}` : label;
  return (
    <button
      type="button"
      className={`update-control update-control--${state}`}
      onClick={() => void handleUpdate()}
      disabled={disabled}
      aria-label={appVersion ? `v${appVersion} · ${label}` : label}
      title={title}
    >
      {state === "downloading" || state === "checking" ? (
        <RefreshCw size={15} className="update-control__spin" aria-hidden="true" />
      ) : (
        <Download size={15} aria-hidden="true" />
      )}
      {appVersion && (
        <span className="update-control__version" aria-hidden="true">v{appVersion}</span>
      )}
      <span className="update-control__label">{label}</span>
    </button>
  );
}
