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

const SORT_OPTIONS: { value: DirFileSort; key: "fileManager.toolbar.sortOptions.nameAsc" | "fileManager.toolbar.sortOptions.nameDesc" | "fileManager.toolbar.sortOptions.sizeDesc" | "fileManager.toolbar.sortOptions.sizeAsc" | "fileManager.toolbar.sortOptions.modifiedDesc" | "fileManager.toolbar.sortOptions.modifiedAsc" }[] = [
  { value: "nameAsc", key: "fileManager.toolbar.sortOptions.nameAsc" },
  { value: "nameDesc", key: "fileManager.toolbar.sortOptions.nameDesc" },
  { value: "sizeDesc", key: "fileManager.toolbar.sortOptions.sizeDesc" },
  { value: "sizeAsc", key: "fileManager.toolbar.sortOptions.sizeAsc" },
  { value: "modifiedDesc", key: "fileManager.toolbar.sortOptions.modifiedDesc" },
  { value: "modifiedAsc", key: "fileManager.toolbar.sortOptions.modifiedAsc" },
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
          {t("fileManager.toolbar.selectedCount", { count: checkedCount })}
          <button type="button" className="fm-toolbar-clear-btn" onClick={onClearChecked}>
            {t("fileManager.toolbar.clear")}
          </button>
        </span>
      ) : (
        <span className="fm-toolbar-count">{t("fileManager.toolbar.filesCount", { count: totalFiles })}</span>
      )}

      <div className="fm-toolbar-right">
        {/* Batch operation buttons — shown when items are checked */}
        {hasBatchSelection && (
          <div className="fm-toolbar-batch-actions">
            <button
              type="button"
              className="fm-toolbar-btn fm-toolbar-btn--danger"
              onClick={onTrashChecked}
              title={t("fileManager.toolbar.trashTitle")}
            >
              <Trash2 size={13} />
              {t("fileManager.toolbar.trashLabel")}
            </button>
            <button
              type="button"
              className="fm-toolbar-btn"
              onClick={onMoveChecked}
              title={t("fileManager.toolbar.moveTitle")}
            >
              <FolderInput size={13} />
              {t("fileManager.toolbar.moveLabel")}
            </button>
            <button
              type="button"
              className="fm-toolbar-btn"
              onClick={onCopyChecked}
              title={t("fileManager.toolbar.copyTitle")}
            >
              <Copy size={13} />
              {t("fileManager.toolbar.copyLabel")}
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
              title={t("fileManager.toolbar.openTitle")}
            >
              {t("fileManager.toolbar.openLabel")}
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
            title={t("fileManager.toolbar.undoTitle")}
            aria-label={t("fileManager.toolbar.undoAria")}
          >
            <RotateCcw size={13} />
          </button>
        )}

        {/* Organize wizard trigger */}
        <button
          type="button"
          className="fm-toolbar-btn"
          onClick={onOrganize}
          title={t("fileManager.toolbar.organizeTitle")}
        >
          <Wand2 size={13} />
          {t("fileManager.toolbar.organizeLabel")}
        </button>

        {/* Hidden files toggle */}
        <button
          type="button"
          className={`fm-toolbar-icon-btn${includeHidden ? " fm-toolbar-icon-btn--active" : ""}`}
          onClick={() => onIncludeHiddenChange(!includeHidden)}
          title={includeHidden ? t("fileManager.toolbar.hideHidden") : t("fileManager.toolbar.showHidden")}
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
            aria-label={t("fileManager.toolbar.sortAria")}
          >
            {SORT_OPTIONS.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {t(opt.key)}
              </option>
            ))}
          </select>
        </div>

        {/* View toggle */}
        <div className="fm-toolbar-view-toggle" role="group" aria-label={t("fileManager.aria.viewMode")}>
          <button
            type="button"
            className={`fm-toolbar-icon-btn${view === "list" ? " fm-toolbar-icon-btn--active" : ""}`}
            onClick={() => onViewChange("list")}
            aria-pressed={view === "list"}
            title={t("fileManager.toolbar.listView")}
          >
            <LayoutList size={14} />
          </button>
          <button
            type="button"
            className={`fm-toolbar-icon-btn${view === "grid" ? " fm-toolbar-icon-btn--active" : ""}`}
            onClick={() => onViewChange("grid")}
            aria-pressed={view === "grid"}
            title={t("fileManager.toolbar.gridView")}
          >
            <LayoutGrid size={14} />
          </button>
        </div>
      </div>
    </div>
  );
}
