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
  return (
    <section className="scan-controls" aria-labelledby="scan-title">
      <div className="section-heading">
        <h2 id="scan-title">选择要了解的文件夹</h2>
        <label className="toggle-control">
          包含隐藏文件
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
            <strong>{selectedPath ? basename(selectedPath) : "尚未选择目录"}</strong>
            <span title={selectedPath}>{selectedPath || "从一个明确的目录开始只读扫描"}</span>
          </span>
        </button>

        {isRunning ? (
          <button className="button button-danger" type="button" onClick={onCancel}>
            <Square size={14} fill="currentColor" />
            取消
          </button>
        ) : (
          <button
            className="button button-primary"
            type="button"
            onClick={onStart}
            disabled={!selectedPath}
          >
            <Play size={15} fill="currentColor" />
            扫描
          </button>
        )}
      </div>
    </section>
  );
}
