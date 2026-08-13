import {
  ArrowUpDown,
  Copy,
  Eye,
  EyeOff,
  FolderInput,
  LayoutGrid,
  LayoutList,
  RotateCcw,
  Trash2,
  Wand2,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import type { DirFileSort, FileListView } from "../../types/fileManager";

type Props = {
  sort: DirFileSort;
  view: FileListView;
  includeHidden: boolean;
  totalFiles: number;
  selectedFile: boolean;
  checkedCount: number;
  canUndo: boolean;
  onSortChange: (sort: DirFileSort) => void;
  onViewChange: (view: FileListView) => void;
  onIncludeHiddenChange: (v: boolean) => void;
  onRevealSelected: () => void;
  onOpenSelected: () => void;
  onTrashChecked: () => void;
  onMoveChecked: () => void;
  onCopyChecked: () => void;
  onClearChecked: () => void;
  onUndo: () => void;
  onOrganize: () => void;
};

const SORT_OPTIONS: { value: DirFileSort; labelZh: string }[] = [
  { value: "nameAsc", labelZh: "名称 A–Z" },
  { value: "nameDesc", labelZh: "名称 Z–A" },
  { value: "sizeDesc", labelZh: "大小：从大到小" },
  { value: "sizeAsc", labelZh: "大小：从小到大" },
  { value: "modifiedDesc", labelZh: "修改时间：最近优先" },
  { value: "modifiedAsc", labelZh: "修改时间：最早优先" },
];

export function FileManagerToolbar({
  sort,
  view,
  includeHidden,
  totalFiles,
  selectedFile,
  checkedCount,
  canUndo,
  onSortChange,
  onViewChange,
  onIncludeHiddenChange,
  onRevealSelected,
  onOpenSelected,
  onTrashChecked,
  onMoveChecked,
  onCopyChecked,
  onClearChecked,
  onUndo,
  onOrganize,
}: Props) {
  const { t } = useTranslation();
  const hasBatchSelection = checkedCount > 0;

  return (
    <div className="fm-toolbar">
      {/* Left: file count or selection count */}
      {hasBatchSelection ? (
        <span className="fm-toolbar-selection-count">
          已选 {checkedCount} 项
          <button type="button" className="fm-toolbar-clear-btn" onClick={onClearChecked}>
            清除
          </button>
        </span>
      ) : (
        <span className="fm-toolbar-count">{totalFiles} 个文件</span>
      )}

      <div className="fm-toolbar-right">
        {/* Batch operation buttons — shown when items are checked */}
        {hasBatchSelection && (
          <div className="fm-toolbar-batch-actions">
            <button
              type="button"
              className="fm-toolbar-btn fm-toolbar-btn--danger"
              onClick={onTrashChecked}
              title="移至废纸篓"
            >
              <Trash2 size={13} />
              废纸篓
            </button>
            <button
              type="button"
              className="fm-toolbar-btn"
              onClick={onMoveChecked}
              title="移动到…"
            >
              <FolderInput size={13} />
              移动
            </button>
            <button
              type="button"
              className="fm-toolbar-btn"
              onClick={onCopyChecked}
              title="复制到…"
            >
              <Copy size={13} />
              复制
            </button>
          </div>
        )}

        {/* Single-file actions from preview selection */}
        {selectedFile && !hasBatchSelection && (
          <div className="fm-toolbar-actions">
            <button
              type="button"
              className="fm-toolbar-btn"
              onClick={onOpenSelected}
              title="用默认应用打开"
            >
              打开
            </button>
            <button
              type="button"
              className="fm-toolbar-btn"
              onClick={onRevealSelected}
              title={t("common.reveal")}
            >
              {t("common.reveal")}
            </button>
          </div>
        )}

        {/* Undo */}
        {canUndo && (
          <button
            type="button"
            className="fm-toolbar-icon-btn"
            onClick={onUndo}
            title="撤销上一步操作"
            aria-label="撤销"
          >
            <RotateCcw size={13} />
          </button>
        )}

        {/* Organize wizard trigger */}
        <button
          type="button"
          className="fm-toolbar-btn"
          onClick={onOrganize}
          title="智能整理"
        >
          <Wand2 size={13} />
          整理
        </button>

        {/* Hidden files toggle */}
        <button
          type="button"
          className={`fm-toolbar-icon-btn${includeHidden ? " fm-toolbar-icon-btn--active" : ""}`}
          onClick={() => onIncludeHiddenChange(!includeHidden)}
          title={includeHidden ? "隐藏隐藏文件" : "显示隐藏文件"}
          aria-pressed={includeHidden}
        >
          {includeHidden ? <Eye size={14} /> : <EyeOff size={14} />}
        </button>

        {/* Sort */}
        <div className="fm-toolbar-sort">
          <ArrowUpDown size={12} className="fm-toolbar-sort-icon" />
          <select
            value={sort}
            onChange={(e) => onSortChange(e.target.value as DirFileSort)}
            aria-label="排序方式"
          >
            {SORT_OPTIONS.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.labelZh}
              </option>
            ))}
          </select>
        </div>

        {/* View toggle */}
        <div className="fm-toolbar-view-toggle" role="group" aria-label="视图模式">
          <button
            type="button"
            className={`fm-toolbar-icon-btn${view === "list" ? " fm-toolbar-icon-btn--active" : ""}`}
            onClick={() => onViewChange("list")}
            aria-pressed={view === "list"}
            title="列表视图"
          >
            <LayoutList size={14} />
          </button>
          <button
            type="button"
            className={`fm-toolbar-icon-btn${view === "grid" ? " fm-toolbar-icon-btn--active" : ""}`}
            onClick={() => onViewChange("grid")}
            aria-pressed={view === "grid"}
            title="网格视图"
          >
            <LayoutGrid size={14} />
          </button>
        </div>
      </div>
    </div>
  );
}
