import { useTranslation } from "react-i18next";
import { FolderOpen, Play, Square } from "lucide-react";
import { basename } from "../lib/format";

type ScanControlsProps = {
  selectedPath: string;
  includeHidden: boolean;
  isRunning: boolean;
  onChoose: () => void;
  onStart: () => void;
  onCancel: () => void;
  onIncludeHiddenChange: (value: boolean) => void;
};

export function ScanControls({
  selectedPath,
  includeHidden,
  isRunning,
  onChoose,
  onStart,
  onCancel,
  onIncludeHiddenChange,
}: ScanControlsProps) {
  const { t } = useTranslation();
  return (
    <section className="scan-controls" aria-labelledby="scan-title">
      <div className="section-heading">
        <h2 id="scan-title">{t("scanControls.title")}</h2>
        <label className="toggle-control">
          {t("scanControls.includeHidden")}
          <input
            type="checkbox"
            checked={includeHidden}
            disabled={isRunning}
            onChange={(event) => onIncludeHiddenChange(event.target.checked)}
          />
          <span className="switch" aria-hidden="true" />
        </label>
      </div>

      <div className="path-picker">
        <button
          className="path-button"
          type="button"
          onClick={onChoose}
          disabled={isRunning}
        >
          <FolderOpen size={19} />
          <span className="path-copy">
            <strong>{selectedPath ? basename(selectedPath) : t("scanControls.notSelected")}</strong>
            <span title={selectedPath}>{selectedPath || t("scanControls.startHint")}</span>
          </span>
        </button>

        {isRunning ? (
          <button className="button button-danger" type="button" onClick={onCancel}>
            <Square size={14} fill="currentColor" />
            {t("scanControls.cancel")}
          </button>
        ) : (
          <button
            className="button button-primary"
            type="button"
            onClick={onStart}
            disabled={!selectedPath}
          >
            <Play size={15} fill="currentColor" />
            {t("scanControls.scan")}
          </button>
        )}
      </div>
    </section>
  );
}
