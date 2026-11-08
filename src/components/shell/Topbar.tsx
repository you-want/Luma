// Topbar — the contextual top bar. One component, three modes:
//   workspace  directory pill + scan status + hidden toggle + scan/cancel
//   machine    page title + whole-machine privacy note (cleaner, sessions)
//   plain      page title + note (history, settings)
// The global cluster (local storage, updates, language) renders in all modes.

import { useTranslation } from "react-i18next";
import { Eye, EyeOff, FolderOpen, Play, RefreshCw, Square } from "lucide-react";
import { basename } from "../../lib/format";
import { ButtonSpinner } from "./ButtonSpinner";
import { IndexStorage } from "./IndexStorage";
import { LanguageSwitcher } from "./LanguageSwitcher";
import { UpdateButton } from "./UpdateButton";
import type { AppView, TopbarMode } from "../../types/navigation";
import type { ScanActionPending } from "../../hooks/useScanActions";

type TopbarProps = {
  mode: TopbarMode;
  view: AppView;
  selectedPath: string;
  includeHidden: boolean;
  isRunning: boolean;
  loading: boolean;
  hasResult: boolean;
  /** Composed scan/operation status line owned by the app shell. */
  statusText: string;
  fileOperationPending: boolean;
  storagePending: boolean;
  pendingAction: ScanActionPending | null;
  onChoose: () => void;
  onIncludeHiddenChange: (value: boolean) => void;
  onStart: () => void;
  onCancel: () => void;
  onIndexCleared: () => void;
  onIndexWorkingChange: (working: boolean) => void;
};

export function Topbar({
  mode,
  view,
  selectedPath,
  includeHidden,
  isRunning,
  loading,
  hasResult,
  statusText,
  fileOperationPending,
  storagePending,
  pendingAction,
  onChoose,
  onIncludeHiddenChange,
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
        <label className="toggle-control topbar-hidden-toggle" title={t("workspace.hiddenHint")}>
          <span aria-hidden="true">{includeHidden ? <Eye size={14} /> : <EyeOff size={14} />}</span>
          <span className="visually-hidden">{t("scanControls.includeHidden")}</span>
          <input type="checkbox" checked={includeHidden} disabled={scanControlsDisabled}
            onChange={event => onIncludeHiddenChange(event.target.checked)} />
          <span className="switch" aria-hidden="true" />
        </label>
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
        <UpdateButton />
        <LanguageSwitcher />
      </div>
    </header>
  );
}
