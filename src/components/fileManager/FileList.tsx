import {
  ChevronRight,
  File,
  FileImage,
  FileText,
  FileVideo,
  FileAudio,
  FileArchive,
  FileCode,
  Folder,
  Pencil,
  Trash2,
} from "lucide-react";
import type { DirNode } from "../../types/fileManager";
import type { FileEntry } from "../../types/scan";
import { formatBytes, formatDate } from "../../lib/format";
import { RenameInput } from "./RenameInput";

// ── Icon helper ────────────────────────────────────────────────────────────────

function FileIcon({ category, size = 16 }: { category: string; size?: number }) {
  switch (category) {
    case "images":    return <FileImage size={size} />;
    case "videos":    return <FileVideo size={size} />;
    case "audio":     return <FileAudio size={size} />;
    case "documents": return <FileText size={size} />;
    case "code":      return <FileCode size={size} />;
    case "archives":  return <FileArchive size={size} />;
    default:          return <File size={size} />;
  }
}

// ── Dir row ────────────────────────────────────────────────────────────────────

type DirRowProps = {
  dir: DirNode;
  onNavigate: (path: string) => void;
};

function DirRow({ dir, onNavigate }: DirRowProps) {
  return (
    <button
      type="button"
      className="fm-file-row fm-file-row--dir"
      onClick={() => onNavigate(dir.path)}
      title={dir.path}
    >
      <span className="fm-file-checkbox-col" />
      <span className="fm-file-icon fm-file-icon--dir">
        <Folder size={16} />
      </span>
      <span className="fm-file-name">{dir.name}</span>
      <span className="fm-file-date fm-file-meta">{dir.fileCount} 项</span>
      <span className="fm-file-size">{formatBytes(dir.sizeBytes)}</span>
      <ChevronRight size={12} className="fm-file-chevron" aria-hidden />
    </button>
  );
}

// ── File row ───────────────────────────────────────────────────────────────────

type FileRowProps = {
  file: FileEntry;
  isSelected: boolean;
  isChecked: boolean;
  isRenaming: boolean;
  onSelect: (file: FileEntry) => void;
  onToggleCheck: (path: string) => void;
  onReveal: (path: string) => void;
  onOpen: (path: string) => void;
  onStartRename: (path: string) => void;
  onCommitRename: (oldPath: string, newName: string) => void;
  onCancelRename: () => void;
  onTrashOne: (path: string) => void;
};

function FileRow({
  file,
  isSelected,
  isChecked,
  isRenaming,
  onSelect,
  onToggleCheck,
  onReveal,
  onOpen,
  onStartRename,
  onCommitRename,
  onCancelRename,
  onTrashOne,
}: FileRowProps) {
  const rowClass = [
    "fm-file-row",
    isSelected ? "fm-file-row--selected" : "",
    isChecked  ? "fm-file-row--checked"  : "",
  ].filter(Boolean).join(" ");

  return (
    <div
      className={rowClass}
      onClick={() => onSelect(file)}
      onDoubleClick={() => onOpen(file.path)}
      role="row"
      aria-selected={isSelected}
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === "Enter") onOpen(file.path);
        if (e.key === " ")     { e.preventDefault(); onSelect(file); }
        if (e.key === "F2")    onStartRename(file.path);
        if (e.key === "Delete" || e.key === "Backspace") onTrashOne(file.path);
      }}
    >
      {/* Checkbox */}
      <span
        className="fm-file-checkbox-col"
        onClick={(e) => { e.stopPropagation(); onToggleCheck(file.path); }}
      >
        <input
          type="checkbox"
          className="fm-file-checkbox"
          checked={isChecked}
          onChange={() => onToggleCheck(file.path)}
          aria-label={`选择 ${file.name}`}
          tabIndex={-1}
          onClick={(e) => e.stopPropagation()}
        />
      </span>

      <span className={`fm-file-icon fm-file-icon--${file.category}`}>
        <FileIcon category={file.category} />
      </span>

      {/* Inline rename input or normal name */}
      {isRenaming ? (
        <RenameInput
          initialName={file.name}
          onConfirm={(newName) => onCommitRename(file.path, newName)}
          onCancel={onCancelRename}
        />
      ) : (
        <span className="fm-file-name" title={file.name}>{file.name}</span>
      )}

      <span className="fm-file-date">{formatDate(file.modifiedAt)}</span>
      <span className="fm-file-size">{formatBytes(file.sizeBytes)}</span>

      {/* Row-level action buttons (appear on hover) */}
      <span className="fm-file-row-actions">
        <button
          type="button"
          className="icon-button fm-file-action-btn"
          onClick={(e) => { e.stopPropagation(); onStartRename(file.path); }}
          title="重命名"
          tabIndex={-1}
          aria-label={`重命名 ${file.name}`}
        >
          <Pencil size={12} />
        </button>
        <button
          type="button"
          className="icon-button fm-file-action-btn fm-file-action-btn--danger"
          onClick={(e) => { e.stopPropagation(); onTrashOne(file.path); }}
          title="移至废纸篓"
          tabIndex={-1}
          aria-label={`将 ${file.name} 移至废纸篓`}
        >
          <Trash2 size={12} />
        </button>
        <button
          type="button"
          className="fm-file-reveal icon-button"
          onClick={(e) => { e.stopPropagation(); onReveal(file.path); }}
          title="在 Finder 中显示"
          tabIndex={-1}
          aria-label={`在 Finder 中显示 ${file.name}`}
        >
          <ChevronRight size={13} />
        </button>
      </span>
    </div>
  );
}

// ── Grid cell ──────────────────────────────────────────────────────────────────

type GridCellProps = {
  file: FileEntry;
  isSelected: boolean;
  isChecked: boolean;
  onSelect: (file: FileEntry) => void;
  onToggleCheck: (path: string) => void;
  onOpen: (path: string) => void;
};

function GridCell({ file, isSelected, isChecked, onSelect, onToggleCheck, onOpen }: GridCellProps) {
  return (
    <div
      className={`fm-grid-cell${isSelected ? " fm-grid-cell--selected" : ""}${isChecked ? " fm-grid-cell--checked" : ""}`}
      onClick={() => onSelect(file)}
      onDoubleClick={() => onOpen(file.path)}
      role="gridcell"
      aria-selected={isSelected}
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === "Enter") onOpen(file.path);
        if (e.key === " ")     { e.preventDefault(); onSelect(file); }
      }}
    >
      <span
        className="fm-grid-checkbox"
        onClick={(e) => { e.stopPropagation(); onToggleCheck(file.path); }}
      >
        <input
          type="checkbox"
          checked={isChecked}
          onChange={() => onToggleCheck(file.path)}
          aria-label={`选择 ${file.name}`}
          tabIndex={-1}
          onClick={(e) => e.stopPropagation()}
        />
      </span>
      <span className={`fm-grid-icon fm-file-icon--${file.category}`}>
        <FileIcon category={file.category} size={24} />
      </span>
      <span className="fm-grid-name" title={file.name}>{file.name}</span>
      <span className="fm-grid-size">{formatBytes(file.sizeBytes)}</span>
    </div>
  );
}

// ── Header row ─────────────────────────────────────────────────────────────────

type HeaderProps = {
  allChecked: boolean;
  someChecked: boolean;
  onToggleAll: () => void;
};

function ListHeader({ allChecked, someChecked, onToggleAll }: HeaderProps) {
  return (
    <div className="fm-file-row fm-file-row--header" role="row">
      <span className="fm-file-checkbox-col">
        <input
          type="checkbox"
          className="fm-file-checkbox"
          checked={allChecked}
          ref={(el) => { if (el) el.indeterminate = someChecked && !allChecked; }}
          onChange={onToggleAll}
          aria-label="全选当前页"
        />
      </span>
      <span />
      <span>名称</span>
      <span>修改时间</span>
      <span>大小</span>
      <span />
    </div>
  );
}

// ── Pagination ─────────────────────────────────────────────────────────────────

type PaginationProps = {
  page: number;
  totalPages: number;
  onPageChange: (p: number) => void;
};

function Pagination({ page, totalPages, onPageChange }: PaginationProps) {
  if (totalPages <= 1) return null;
  return (
    <div className="fm-pagination">
      <button type="button" className="button" disabled={page === 0} onClick={() => onPageChange(page - 1)}>
        上一页
      </button>
      <span className="fm-pagination-info">第 {page + 1} / {totalPages} 页</span>
      <button type="button" className="button" disabled={page >= totalPages - 1} onClick={() => onPageChange(page + 1)}>
        下一页
      </button>
    </div>
  );
}

// ── FileList (root export) ─────────────────────────────────────────────────────

type Props = {
  dirs: DirNode[];
  files: FileEntry[];
  selectedFile: FileEntry | null;
  checkedPaths: Set<string>;
  renamingPath: string | null;
  view: "list" | "grid";
  page: number;
  totalPages: number;
  onNavigate: (path: string) => void;
  onSelectFile: (file: FileEntry | null) => void;
  onToggleCheck: (path: string) => void;
  onReveal: (path: string) => void;
  onOpen: (path: string) => void;
  onPageChange: (p: number) => void;
  onStartRename: (path: string) => void;
  onCommitRename: (oldPath: string, newName: string) => void;
  onCancelRename: () => void;
  onTrashOne: (path: string) => void;
};

export function FileList({
  dirs,
  files,
  selectedFile,
  checkedPaths,
  renamingPath,
  view,
  page,
  totalPages,
  onNavigate,
  onSelectFile,
  onToggleCheck,
  onReveal,
  onOpen,
  onPageChange,
  onStartRename,
  onCommitRename,
  onCancelRename,
  onTrashOne,
}: Props) {
  const isEmpty = dirs.length === 0 && files.length === 0;
  const allChecked = files.length > 0 && files.every((f) => checkedPaths.has(f.path));
  const someChecked = files.some((f) => checkedPaths.has(f.path));

  function handleToggleAll() {
    if (allChecked) {
      files.forEach((f) => checkedPaths.has(f.path) && onToggleCheck(f.path));
    } else {
      files.forEach((f) => !checkedPaths.has(f.path) && onToggleCheck(f.path));
    }
  }

  if (isEmpty) {
    return (
      <div className="fm-list-empty">
        <Folder size={28} />
        <p>此目录为空</p>
      </div>
    );
  }

  return (
    <div className="fm-file-list">
      {/* Directories — always list style */}
      {dirs.length > 0 && (
        <div className="fm-file-dirs">
          {dirs.map((dir) => (
            <DirRow key={dir.path} dir={dir} onNavigate={onNavigate} />
          ))}
        </div>
      )}

      {/* Files */}
      {files.length > 0 && (
        view === "grid" ? (
          <div className="fm-grid" role="grid" aria-label="文件网格">
            {files.map((f) => (
              <GridCell
                key={f.id}
                file={f}
                isSelected={selectedFile?.id === f.id}
                isChecked={checkedPaths.has(f.path)}
                onSelect={onSelectFile}
                onToggleCheck={onToggleCheck}
                onOpen={onOpen}
              />
            ))}
          </div>
        ) : (
          <div className="fm-file-rows" role="rowgroup">
            <ListHeader
              allChecked={allChecked}
              someChecked={someChecked}
              onToggleAll={handleToggleAll}
            />
            {files.map((f) => (
              <FileRow
                key={f.id}
                file={f}
                isSelected={selectedFile?.id === f.id}
                isChecked={checkedPaths.has(f.path)}
                isRenaming={renamingPath === f.path}
                onSelect={onSelectFile}
                onToggleCheck={onToggleCheck}
                onReveal={onReveal}
                onOpen={onOpen}
                onStartRename={onStartRename}
                onCommitRename={onCommitRename}
                onCancelRename={onCancelRename}
                onTrashOne={onTrashOne}
              />
            ))}
          </div>
        )
      )}

      <Pagination page={page} totalPages={totalPages} onPageChange={onPageChange} />
    </div>
  );
}
