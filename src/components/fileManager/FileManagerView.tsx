// FileManagerView — top-level layout shell.
// Composes DirectoryTree + FileList + FilePreviewPanel via useFileManager + useFileOps.

import { AlertTriangle } from "lucide-react";
import { useCallback, useState } from "react";
import { Breadcrumb } from "./Breadcrumb";
import { ConfirmDialog } from "./ConfirmDialog";
import { DirectoryTree } from "./DirectoryTree";
import { FileList } from "./FileList";
import { FileManagerToolbar } from "./FileManagerToolbar";
import { FilePreviewPanel } from "./FilePreviewPanel";
import { OrganizeWizard } from "./OrganizeWizard";
import { ToastStack, useToasts } from "./ToastStack";
import { useFileManager } from "./useFileManager";
import { useFileOps } from "./useFileOps";
import { chooseDirectory, getDirectoryNodes } from "../../lib/tauri";
import type { DirNode } from "../../types/fileManager";
import "./FileManager.css";

type Props = {
  scanId: string;
  rootPath: string;
};

export function FileManagerView({ scanId, rootPath }: Props) {
  const { toasts, push: pushToast, dismiss: dismissToast } = useToasts();

  const fm = useFileManager(scanId, rootPath);

  // File operations hook — needs a refresh callback and a toast emitter
  const ops = useFileOps(scanId, () => fm.reload(), pushToast);

  // Confirm-trash dialog state
  const [trashPending, setTrashPending] = useState<string[] | null>(null);

  // Organize wizard state
  const [organizeOpen, setOrganizeOpen] = useState(false);

  // Lazy-load children for the tree sidebar
  const loadChildren = useCallback(
    async (path: string): Promise<DirNode[]> => {
      try {
        return await getDirectoryNodes(scanId, path);
      } catch {
        return [];
      }
    },
    [scanId],
  );

  // ── Trash flow: show confirmation, then execute ───────────────────────────

  function requestTrash(paths: string[]) {
    if (paths.length === 0) return;
    setTrashPending(paths);
  }

  function confirmTrash() {
    if (!trashPending) return;
    const paths = trashPending;
    setTrashPending(null);
    void ops.trash(paths);
  }

  // ── Move / copy: use OS directory picker for destination ─────────────────

  async function handleMoveChecked() {
    const dest = await chooseDirectory();
    if (!dest) return;
    void ops.move([...ops.checkedPaths], dest);
  }

  async function handleCopyChecked() {
    const dest = await chooseDirectory();
    if (!dest) return;
    void ops.copy([...ops.checkedPaths], dest);
  }

  // ── Render ────────────────────────────────────────────────────────────────

  if (fm.status === "idle" || fm.status === "loading") {
    return (
      <div className="fm-loading-state">
        <span className="fm-spinner" aria-hidden />
        <span>正在读取目录…</span>
      </div>
    );
  }

  if (fm.status === "error") {
    return (
      <div className="fm-error-state">
        <AlertTriangle size={20} />
        <p>{fm.error ?? "读取目录失败"}</p>
      </div>
    );
  }

  return (
    <div className="fm-view">
      {/* Breadcrumb navigation bar */}
      <div className="fm-nav-bar">
        <Breadcrumb segments={fm.breadcrumbs} onNavigate={fm.navigate} />
      </div>

      {/* Toolbar */}
      <FileManagerToolbar
        sort={fm.sort}
        view={fm.view}
        includeHidden={fm.includeHidden}
        totalFiles={fm.totalFiles}
        selectedFile={fm.selected !== null}
        checkedCount={ops.checkedPaths.size}
        canUndo={ops.canUndo}
        onSortChange={fm.setSort}
        onViewChange={fm.setView}
        onIncludeHiddenChange={fm.setIncludeHidden}
        onRevealSelected={() => fm.selected && fm.handleReveal(fm.selected.path)}
        onOpenSelected={() => fm.selected && fm.handleOpen(fm.selected.path)}
        onTrashChecked={() => requestTrash([...ops.checkedPaths])}
        onMoveChecked={handleMoveChecked}
        onCopyChecked={handleCopyChecked}
        onClearChecked={ops.clearChecked}
        onUndo={() => void ops.undoLast()}
        onOrganize={() => setOrganizeOpen(true)}
      />

      {/* Body: tree sidebar + file list + preview panel */}
      <div className="fm-body">
        <aside className="fm-sidebar">
          <DirectoryTree
            rootDirs={fm.dirs}
            currentPath={fm.currentPath}
            scanId={scanId}
            onNavigate={fm.navigate}
            onLoadChildren={loadChildren}
          />
        </aside>

        <main className="fm-main">
          <FileList
            dirs={fm.dirs}
            files={fm.files}
            selectedFile={fm.selected}
            checkedPaths={ops.checkedPaths}
            renamingPath={ops.renamingPath}
            view={fm.view}
            page={fm.page}
            totalPages={fm.totalPages}
            onNavigate={fm.navigate}
            onSelectFile={fm.selectFile}
            onToggleCheck={ops.toggleCheck}
            onReveal={fm.handleReveal}
            onOpen={fm.handleOpen}
            onPageChange={fm.setPage}
            onStartRename={ops.startRename}
            onCommitRename={ops.commitRename}
            onCancelRename={ops.cancelRename}
            onTrashOne={(path) => requestTrash([path])}
          />
        </main>

        <FilePreviewPanel
          file={fm.selected}
          onClose={() => fm.selectFile(null)}
          onReveal={fm.handleReveal}
          onOpen={fm.handleOpen}
        />
      </div>

      {/* Confirmation dialog */}
      <ConfirmDialog
        open={trashPending !== null}
        title="移至废纸篓"
        message={
          trashPending && trashPending.length === 1
            ? `确认将"${trashPending[0].split("/").pop()}"移至废纸篓？`
            : `确认将 ${trashPending?.length ?? 0} 个文件移至废纸篓？`
        }
        confirmLabel="移至废纸篓"
        variant="danger"
        onConfirm={confirmTrash}
        onCancel={() => setTrashPending(null)}
      />

      {/* Toast notifications */}
      <ToastStack toasts={toasts} onDismiss={dismissToast} />

      {/* Organize wizard */}
      <OrganizeWizard
        open={organizeOpen}
        scanId={scanId}
        defaultSourceDir={fm.currentPath}
        onClose={() => setOrganizeOpen(false)}
        onDone={() => { fm.reload(); setOrganizeOpen(false); }}
      />
    </div>
  );
}
