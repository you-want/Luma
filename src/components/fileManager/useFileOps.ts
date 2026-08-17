// useFileOps — all file operation logic (trash, rename, move, copy) plus
// the persisted SQLite-backed undo stack.

import { useCallback, useEffect, useState } from "react";
import {
  trashFiles,
  renameFile,
  moveFiles,
  copyFiles,
  listUndoableOperations,
  undoFileOperation,
} from "../../lib/tauri";
import { errorMessage } from "../../lib/errors";
import type { OperationRecord } from "../../types/fileManager";
import type { FileEntry } from "../../types/scan";

export type OpsState = {
  /** Paths selected for batch operations (not the same as preview selection). */
  checkedPaths: Set<string>;
  undoStack: OperationRecord[];
  /** Path currently being renamed (shows inline input). */
  renamingPath: string | null;
};

export type FileOpsHook = ReturnType<typeof useFileOps>;

type PushToast = (opts: {
  kind: "success" | "error" | "info";
  message: string;
  undoLabel?: string;
  onUndo?: () => void;
}) => void;

export function useFileOps(
  scanId: string,
  onRefresh: () => void,
  pushToast: PushToast,
) {
  const [checkedPaths, setCheckedPaths] = useState<Set<string>>(new Set());
  const [undoStack, setUndoStack] = useState<OperationRecord[]>([]);
  const [renamingPath, setRenamingPath] = useState<string | null>(null);

  // ── Checkbox selection ─────────────────────────────────────────────────────

  const toggleCheck = useCallback((path: string) => {
    setCheckedPaths((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, []);

  const checkAll = useCallback((files: FileEntry[]) => {
    setCheckedPaths(new Set(files.map((f) => f.path)));
  }, []);

  const clearChecked = useCallback(() => {
    setCheckedPaths(new Set());
  }, []);

  // ── Persistent undo helpers ────────────────────────────────────────────────

  const refreshUndoable = useCallback(async () => {
    try {
      setUndoStack(await listUndoableOperations(scanId));
    } catch {
      setUndoStack([]);
    }
  }, [scanId]);

  useEffect(() => {
    void refreshUndoable();
  }, [refreshUndoable]);

  const undoById = useCallback(
    async (operationId: string, label: string) => {
      try {
        const result = await undoFileOperation(operationId, scanId);
        onRefresh();
        await refreshUndoable();
        if (result.failed.length === 0) {
          pushToast({ kind: "info", message: `已撤销：${label}` });
        } else {
          pushToast({
            kind: "error",
            message: `部分项目撤销失败：${result.failed[0]?.reason ?? "未知错误"}`,
          });
        }
      } catch (err) {
        pushToast({ kind: "error", message: `撤销失败：${errorMessage(err)}` });
        await refreshUndoable();
      }
    },
    [onRefresh, pushToast, refreshUndoable, scanId],
  );

  // ── Trash ──────────────────────────────────────────────────────────────────

  const trash = useCallback(
    async (paths: string[]) => {
      try {
        const result = await trashFiles(scanId, paths);
        const n = result.succeeded.length;
        if (n > 0) {
          clearChecked();
          onRefresh();
          pushToast({
            kind: "success",
            message: `已将 ${n} 个文件移至废纸篓`,
          });
        }
        for (const f of result.failed) {
          pushToast({ kind: "error", message: `移至废纸篓失败：${f.reason}` });
        }
      } catch (err) {
        pushToast({ kind: "error", message: errorMessage(err) });
      }
    },
    [scanId, onRefresh, clearChecked, pushToast],
  );

  // ── Rename ─────────────────────────────────────────────────────────────────

  const startRename = useCallback((path: string) => {
    setRenamingPath(path);
  }, []);

  const cancelRename = useCallback(() => {
    setRenamingPath(null);
  }, []);

  const commitRename = useCallback(
    async (oldPath: string, newName: string) => {
      setRenamingPath(null);
      try {
        const result = await renameFile(scanId, oldPath, newName);
        onRefresh();
        await refreshUndoable();
        pushToast({
          kind: "success",
          message: `已重命名为 "${newName}"`,
          undoLabel: "撤销",
          onUndo: () => void undoById(result.operationId, `重命名为 ${newName}`),
        });
      } catch (err) {
        pushToast({ kind: "error", message: `重命名失败：${errorMessage(err)}` });
      }
    },
    [scanId, onRefresh, pushToast, refreshUndoable, undoById],
  );

  // ── Move ───────────────────────────────────────────────────────────────────

  const move = useCallback(
    async (paths: string[], destDir: string) => {
      try {
        const result = await moveFiles(scanId, paths, destDir);
        const n = result.succeeded.length;
        if (n > 0) {
          clearChecked();
          onRefresh();
          await refreshUndoable();
          pushToast({
            kind: "success",
            message: `已移动 ${n} 个文件`,
            undoLabel: result.operationId ? "撤销" : undefined,
            onUndo: result.operationId
              ? () => void undoById(result.operationId!, `移动 ${n} 个文件`)
              : undefined,
          });
        }
        for (const f of result.failed) {
          pushToast({ kind: "error", message: `移动失败：${f.reason}` });
        }
      } catch (err) {
        pushToast({ kind: "error", message: errorMessage(err) });
      }
    },
    [scanId, onRefresh, clearChecked, pushToast, refreshUndoable, undoById],
  );

  // ── Copy ───────────────────────────────────────────────────────────────────

  const copy = useCallback(
    async (paths: string[], destDir: string) => {
      try {
        const result = await copyFiles(scanId, paths, destDir);
        const n = result.succeeded.length;
        if (n > 0) {
          clearChecked();
          onRefresh();
          pushToast({ kind: "success", message: `已复制 ${n} 个文件` });
        }
        for (const f of result.failed) {
          pushToast({ kind: "error", message: `复制失败：${f.reason}` });
        }
      } catch (err) {
        pushToast({ kind: "error", message: errorMessage(err) });
      }
    },
    [scanId, onRefresh, clearChecked, pushToast],
  );

  // ── Undo ───────────────────────────────────────────────────────────────────

  const undoLast = useCallback(() => {
    const [entry] = undoStack;
    if (!entry) return;
    void undoById(entry.id, entry.label);
  }, [undoStack, undoById]);

  return {
    checkedPaths,
    undoStack,
    renamingPath,
    toggleCheck,
    checkAll,
    clearChecked,
    startRename,
    cancelRename,
    commitRename,
    trash,
    move,
    copy,
    undoLast,
    refreshUndoable,
    canUndo: undoStack.length > 0,
  };
}
