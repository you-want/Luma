// Topbar — the contextual top bar. One component, three modes:
//   workspace  directory pill + scan status + scan/cancel
//   machine    page title + whole-machine privacy note (cleaner, sessions)
//   plain      page title + note (history, settings)
// The only global control is 本地存储; update checks and language live in
// Settings, and hidden files are toggled in the file manager toolbar and
// Settings (one cryptic eye+switch in the top bar confused users and
// duplicated those affordances).

import { useTranslation } from "react-i18next";
import { FolderOpen, Play, RefreshCw, Square } from "lucide-react";
import { basename } from "../../lib/format";
import { ButtonSpinner } from "./ButtonSpinner";
import { IndexStorage } from "./IndexStorage";
import type { AppView, TopbarMode } from "../../types/navigation";
import type { ScanActionPending } from "../../hooks/useScanActions";

type TopbarProps = {
  mode: TopbarMode;
  view: AppView;
  selectedPath: string;
  isRunning: boolean;
  loading: boolean;
  hasResult: boolean;
  /** Composed scan/operation status line owned by the app shell. */
  statusText: string;
  fileOperationPending: boolean;
  storagePending: boolean;
  pendingAction: ScanActionPending | null;
  onChoose: () => void;
  onStart: () => void;
  onCancel: () => void;
  onIndexCleared: () => void;
  onIndexWorkingChange: (working: boolean) => void;
};

export function Topbar({
  mode,
  view,
  selectedPath,
  isRunning,
  loading,
  hasResult,
  statusText,
  fileOperationPending,
  storagePending,
  pendingAction,
  onChoose,
  onStart,
  onCancel,
  onIndexCleared,
  onIndexWorkingChange,
}: TopbarProps) {
  const { t } = useTranslation();
  const scanControlsDisabled = loading || isRunning || storagePending
    || fileOperationPending || pendingAction !== null;

  return (
    <header className="app-topbar">
      {mode === "workspace" && <div className="topbar-workspace">
        <button type="button" className="topbar-dir-pill" onClick={onChoose}
          disabled={scanControlsDisabled} aria-busy={pendingAction === "choose"}
          title={selectedPath || t("scanControls.startHint")}>
          {pendingAction === "choose" ? <ButtonSpinner size={15} /> : <FolderOpen size={15} aria-hidden="true" />}
          <span className="topbar-dir-name">{selectedPath
            ? basename(selectedPath) || selectedPath
            : t("scanControls.notSelected")}</span>
        </button>
        <p className="topbar-status" role="status">{statusText}</p>
        <span className="topbar-spacer" />
        {isRunning ? (
          <button className="button button-danger topbar-scan-button" type="button" onClick={onCancel}
            disabled={pendingAction !== null} aria-busy={pendingAction === "cancel"}>
            {pendingAction === "cancel" ? <ButtonSpinner /> : <Square size={13} fill="currentColor" aria-hidden="true" />}
            {t(pendingAction === "cancel" ? "scanControls.cancelling" : "scanControls.cancel")}
          </button>
        ) : (
          <button className="button button-primary topbar-scan-button" type="button" onClick={onStart}
            disabled={!selectedPath || scanControlsDisabled} aria-busy={pendingAction === "start"}>
            {pendingAction === "start" ? <ButtonSpinner size={14} />
              : hasResult ? <RefreshCw size={14} aria-hidden="true" />
              : <Play size={14} fill="currentColor" aria-hidden="true" />}
            {t(pendingAction === "start" ? "scanControls.starting" : hasResult ? "workspace.rescan" : "scanControls.scan")}
          </button>
        )}
      </div>}
      {mode !== "workspace" && <div className="topbar-static">
        <strong className="topbar-title">{view === "cleaner" ? t("cleaner.title")
          : view === "sessions" ? t("sessions.title")
          : view === "history" ? t("nav.views.history")
          : t("nav.views.settings")}</strong>
        <span className="topbar-note">{view === "cleaner" ? t("topbar.machineCleaner")
          : view === "sessions" ? t("topbar.machineSessions")
          : view === "history" ? t("topbar.history")
          : t("topbar.settings")}</span>
      </div>}

      <div className="topbar-global">
        <IndexStorage disabled={scanControlsDisabled}
          onCleared={onIndexCleared} onWorkingChange={onIndexWorkingChange} />
      </div>
    </header>
  );
}
