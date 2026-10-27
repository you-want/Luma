import {
  ArrowUpDown,
  Copy,
  Eye,
  EyeOff,
  FolderInput,
  LayoutGrid,
  LayoutList,
  Undo2,
  Trash2,
  Wand2,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import type { DirFileSort, FileListView } from "../../types/fileManager";
import type { FileOperation } from "./useFileOps";
import { ButtonSpinner } from "../ButtonSpinner";

type Props = {
  pendingAction?: FileOperation | "open" | "reveal" | null;
  sort: DirFileSort;
  view: FileListView;
  includeHidden: boolean;
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
  pendingAction = null,
  sort,
  view,
  includeHidden,
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
      <div className="fm-toolbar-leading">
        {/* Organize wizard trigger */}
        <button
          type="button"
          className="fm-toolbar-btn fm-toolbar-btn--organize"
          onClick={onOrganize}
          title={t("fileManager.toolbar.organizeTitle")}
        >
          <Wand2 size={13} />
          {t("fileManager.toolbar.organizeLabel")}
        </button>

        {/* Undo */}
        {canUndo && (
          <button
            type="button"
            className="fm-toolbar-btn"
            onClick={onUndo}
            aria-busy={pendingAction === "undo"}
            title={t("fileManager.toolbar.undoTitle")}
            aria-label={t("fileManager.toolbar.undoAria")}
          >
            {pendingAction === "undo" ? <ButtonSpinner size={13} /> : <Undo2 size={13} />}
            {t("fileManager.toolbar.undoAria")}
          </button>
        )}

        {/* Selection stays beside the actions it applies to. */}
        {hasBatchSelection && (
          <span className="fm-toolbar-selection-count">
            {t("fileManager.toolbar.selectedCount", { count: checkedCount })}
            <button type="button" className="fm-toolbar-clear-btn" onClick={onClearChecked}>
              {t("fileManager.toolbar.clear")}
            </button>
          </span>
        )}

        {/* Batch operation buttons — shown when items are checked */}
        {hasBatchSelection && (
          <div className="fm-toolbar-batch-actions">
            <button
              type="button"
              className="fm-toolbar-btn fm-toolbar-btn--danger"
              onClick={onTrashChecked}
              aria-busy={pendingAction === "trash"}
              title={t("fileManager.toolbar.trashTitle")}
            >
              {pendingAction === "trash" ? <ButtonSpinner size={13} /> : <Trash2 size={13} />}
              {t("fileManager.toolbar.trashLabel")}
            </button>
            <button
              type="button"
              className="fm-toolbar-btn"
              onClick={onMoveChecked}
              aria-busy={pendingAction === "move"}
              title={t("fileManager.toolbar.moveTitle")}
            >
              {pendingAction === "move" ? <ButtonSpinner size={13} /> : <FolderInput size={13} />}
              {t("fileManager.toolbar.moveLabel")}
            </button>
            <button
              type="button"
              className="fm-toolbar-btn"
              onClick={onCopyChecked}
              aria-busy={pendingAction === "copy"}
              title={t("fileManager.toolbar.copyTitle")}
            >
              {pendingAction === "copy" ? <ButtonSpinner size={13} /> : <Copy size={13} />}
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
              aria-busy={pendingAction === "open"}
              title={t("fileManager.toolbar.openTitle")}
            >
              {pendingAction === "open" && <ButtonSpinner size={13} />}
              {t("fileManager.toolbar.openLabel")}
            </button>
            <button
              type="button"
              className="fm-toolbar-btn"
              onClick={onRevealSelected}
              aria-busy={pendingAction === "reveal"}
              title={t("common.reveal")}
            >
              {pendingAction === "reveal" && <ButtonSpinner size={13} />}
              {t("common.reveal")}
            </button>
          </div>
        )}

      </div>

      <div className="fm-toolbar-right">
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
