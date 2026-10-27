// FileManagerView — top-level layout shell.
// Composes DirectoryTree + FileList + FilePreviewPanel via useFileManager + useFileOps.

import { AlertTriangle } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
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
import { errorMessage } from "../../lib/errors";
import { useAsyncAction } from "../../hooks/useAsyncAction";
import { ButtonSpinner } from "../ButtonSpinner";
import type { DirNode } from "../../types/fileManager";
import "./FileManager.css";

type Props = {
  scanId: string;
  rootPath: string;
  onWorkingChange?: (working: boolean) => void;
};

export function FileManagerView({ scanId, rootPath, onWorkingChange }: Props) {
  const { t } = useTranslation();
  const { toasts, push: pushToast, dismiss: dismissToast } = useToasts();

  const fm = useFileManager(scanId, rootPath);
  const [rootDirs, setRootDirs] = useState<DirNode[]>([]);
  const [treeRevision, setTreeRevision] = useState(0);
  const refresh = useCallback(() => {
    fm.reload();
    setTreeRevision(revision => revision + 1);
  }, [fm.reload]);
  useEffect(() => {
    let disposed = false;
    getDirectoryNodes(scanId, rootPath).then(dirs => {
      if (!disposed) setRootDirs(dirs);
    }).catch(() => { if (!disposed) setRootDirs([]); });
    return () => { disposed = true; };
  }, [scanId, rootPath, treeRevision]);

  // File operations hook — needs a refresh callback and a toast emitter
  const ops = useFileOps(scanId, refresh, pushToast, `${scanId}:${fm.currentPath}:${fm.includeHidden}`);
  const action = useAsyncAction<"move" | "copy" | "open" | "reveal">();
  const pendingAction = ops.pendingOperation ?? action.pending;
  const busy = pendingAction !== null || fm.loading;
  const pendingLabel = !ops.pendingOperation && (action.pending === "move" || action.pending === "copy")
    ? "destination" : pendingAction;
  useEffect(() => { onWorkingChange?.(pendingAction !== null); }, [pendingAction, onWorkingChange]);
  useEffect(() => () => { onWorkingChange?.(false); }, [onWorkingChange]);

  function handleOpen(path: string) {
    void action.run("open", async () => {
      try { await fm.handleOpen(path); }
      catch (error) { pushToast({ kind: "error", message: errorMessage(error) }); }
    });
  }
  function handleReveal(path: string) {
    void action.run("reveal", async () => {
      try { await fm.handleReveal(path); }
      catch (error) { pushToast({ kind: "error", message: errorMessage(error) }); }
    });
  }

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
    await action.run("move", async () => {
      try {
        const paths = [...ops.checkedPaths];
        const dest = await chooseDirectory();
        if (dest) await ops.move(paths, dest);
      } catch (error) { pushToast({ kind: "error", message: errorMessage(error) }); }
    });
  }

  async function handleCopyChecked() {
    await action.run("copy", async () => {
      try {
        const paths = [...ops.checkedPaths];
        const dest = await chooseDirectory();
        if (dest) await ops.copy(paths, dest);
      } catch (error) { pushToast({ kind: "error", message: errorMessage(error) }); }
    });
  }

  // ── Render ────────────────────────────────────────────────────────────────

  if (fm.status === "idle" || fm.status === "loading") {
    return (
      <div className="fm-loading-state">
        <span className="fm-spinner" aria-hidden />
        <span>{t("fileManager.state.loading")}</span>
      </div>
    );
  }

  if (fm.status === "error") {
    return (
      <div className="fm-error-state">
        <AlertTriangle size={20} />
        <p>{fm.error ?? t("fileManager.state.loadError")}</p>
      </div>
    );
  }

  return (
    <div className="fm-view" aria-busy={busy}>
      {/* Breadcrumb navigation bar */}
      <div className="fm-nav-bar" inert={busy}>
        <Breadcrumb segments={fm.breadcrumbs} onNavigate={fm.navigate} />
        <span className="fm-toolbar-count">{t("fileManager.toolbar.filesCount", { count: fm.totalFiles })}</span>
      </div>

      {/* Toolbar */}
      <fieldset className="fm-toolbar-fieldset" disabled={busy}>
        <FileManagerToolbar
        pendingAction={pendingAction}
        sort={fm.sort}
        view={fm.view}
        includeHidden={fm.includeHidden}
        selectedFile={fm.selected !== null}
        checkedCount={ops.checkedPaths.size}
        canUndo={ops.canUndo}
        onSortChange={fm.setSort}
        onViewChange={fm.setView}
        onIncludeHiddenChange={fm.setIncludeHidden}
        onRevealSelected={() => fm.selected && handleReveal(fm.selected.path)}
        onOpenSelected={() => fm.selected && handleOpen(fm.selected.path)}
        onTrashChecked={() => requestTrash([...ops.checkedPaths])}
        onMoveChecked={handleMoveChecked}
        onCopyChecked={handleCopyChecked}
        onClearChecked={ops.clearChecked}
        onUndo={() => void ops.undoLast()}
        onOrganize={() => setOrganizeOpen(true)}
      />
      </fieldset>
      {busy && <div className="fm-busy-status" role="status"><ButtonSpinner />{pendingLabel
        ? t(`fileManager.pending.${pendingLabel}`) : t("fileManager.state.loading")}</div>}

      {/* Body: tree sidebar + file list + preview panel */}
      <div className="fm-body" inert={busy}>
        <aside className="fm-sidebar">
          <h2 className="fm-sidebar-heading">{t("fileManager.folders")}</h2>
          <DirectoryTree
            key={`${scanId}:${treeRevision}`}
            rootDirs={rootDirs}
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
            onReveal={handleReveal}
            onOpen={handleOpen}
            onPageChange={fm.setPage}
            onStartRename={ops.startRename}
            onCommitRename={ops.commitRename}
            onCancelRename={ops.cancelRename}
            onTrashOne={(path) => requestTrash([path])}
          />
        </main>

        <FilePreviewPanel
          scanId={scanId}
          pendingAction={pendingAction}
          file={fm.selected}
          onClose={() => fm.selectFile(null)}
          onReveal={handleReveal}
          onOpen={handleOpen}
        />
      </div>

      {/* Confirmation dialog */}
      <ConfirmDialog
        open={trashPending !== null}
        title={t("fileManager.confirmTrash.title")}
        message={
          trashPending && trashPending.length === 1
            ? t("fileManager.confirmTrash.messageOne", { name: trashPending[0].split("/").pop() })
            : t("fileManager.confirmTrash.messageMany", { count: trashPending?.length ?? 0 })
        }
        confirmLabel={t("fileManager.confirmTrash.title")}
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
        onDone={() => {
          refresh();
          void ops.refreshUndoable();
          setOrganizeOpen(false);
        }}
      />
    </div>
  );
}
