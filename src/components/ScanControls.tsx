import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Download, FolderOpen, Monitor, Play, RefreshCw, Square } from "lucide-react";
import { basename } from "../lib/format";
import { ButtonSpinner } from "./ButtonSpinner";

type ScanControlsProps = {
  selectedPath: string;
  includeHidden: boolean;
  isRunning: boolean;
  hasResult?: boolean;
  isLoading?: boolean;
  locked?: boolean;
  statusText?: string;
  children?: ReactNode;
  pendingAction?: "choose" | "desktop" | "downloads" | "start" | "cancel" | null;
  onChoose: () => void;
  onShortcut: (location: "desktop" | "downloads") => void;
  onStart: () => void;
  onCancel: () => void;
  onIncludeHiddenChange: (value: boolean) => void;
};

export function ScanControls({
  selectedPath,
  includeHidden,
  isRunning,
  hasResult = false,
  isLoading = false,
  locked = false,
  statusText,
  children,
  pendingAction = null,
  onChoose,
  onShortcut,
  onStart,
  onCancel,
  onIncludeHiddenChange,
}: ScanControlsProps) {
  const { t } = useTranslation();
  const disabled = isRunning || isLoading || locked || pendingAction !== null;
  return (
    <section className="workspace-controls" aria-labelledby="workspace-title">
      <div className="workspace-location-row">
        <div className="workspace-location">
          <h2 id="workspace-title">{t("workspace.directory")}</h2>
          <button
            className="path-button workspace-path-button"
            type="button"
            onClick={onChoose}
            disabled={disabled}
            aria-busy={pendingAction === "choose"}
            title={selectedPath || t("scanControls.startHint")}
          >
            {pendingAction === "choose" ? <ButtonSpinner size={21} /> : <FolderOpen size={21} />}
            <span className="path-copy">
              <strong>{selectedPath ? basename(selectedPath) || selectedPath : t("scanControls.notSelected")}</strong>
              <span>{selectedPath || t("workspace.chooseHint")}</span>
            </span>
            <span className="workspace-change-label">{t(selectedPath ? "workspace.change" : "workspace.choose")}</span>
          </button>
        </div>
        <div className="workspace-directory-actions" aria-label={t("workspace.quickLocations")}>
          <button type="button" className="button button-secondary" disabled={disabled} aria-busy={pendingAction === "desktop"} onClick={() => onShortcut("desktop")}>
            {pendingAction === "desktop" ? <ButtonSpinner /> : <Monitor size={14} aria-hidden="true" />}
            {t("scanControls.desktop")}
          </button>
          <button type="button" className="button button-secondary" disabled={disabled} aria-busy={pendingAction === "downloads"} onClick={() => onShortcut("downloads")}>
            {pendingAction === "downloads" ? <ButtonSpinner /> : <Download size={14} aria-hidden="true" />}
            {t("scanControls.downloads")}
          </button>
        </div>
        {isRunning ? (
          <button className="button button-danger workspace-scan-button" type="button" onClick={onCancel} disabled={pendingAction !== null} aria-busy={pendingAction === "cancel"}>
            {pendingAction === "cancel" ? <ButtonSpinner /> : <Square size={14} fill="currentColor" />}
            {t(pendingAction === "cancel" ? "scanControls.cancelling" : "scanControls.cancel")}
          </button>
        ) : (
          <button className="button button-primary workspace-scan-button" type="button" onClick={onStart}
            disabled={!selectedPath || disabled} aria-busy={pendingAction === "start"}>
            {pendingAction === "start" ? <ButtonSpinner size={15} /> : hasResult ? <RefreshCw size={15} /> : <Play size={15} fill="currentColor" />}
            {t(pendingAction === "start" ? "scanControls.starting" : hasResult ? "workspace.rescan" : "scanControls.scan")}
          </button>
        )}
      </div>
      <div className="workspace-nav-row">
        <div className="workspace-view-area">
          {children}
          {statusText && <p className="workspace-context" role="status">{statusText}</p>}
        </div>
        <label className="toggle-control workspace-hidden-toggle" title={t("workspace.hiddenHint")}>
          {t("scanControls.includeHidden")}
          <input type="checkbox" checked={includeHidden} disabled={disabled}
            onChange={event => onIncludeHiddenChange(event.target.checked)} />
          <span className="switch" aria-hidden="true" />
        </label>
      </div>
    </section>
  );
}
