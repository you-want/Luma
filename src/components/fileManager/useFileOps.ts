// useFileOps — all file operation logic (trash, rename, move, copy) plus
// in-memory undo stack. Separated from useFileManager so state stays focused.

import { useCallback, useState } from "react";
import { trashFiles, renameFile, moveFiles, copyFiles } from "../../lib/tauri";
import { errorMessage } from "../../lib/errors";
import type { UndoEntry, UndoRecord } from "../../types/fileManager";
import type { FileEntry } from "../../types/scan";

const MAX_UNDO = 20;

export type OpsState = {
  /** Paths selected for batch operations (not the same as preview selection). */
  checkedPaths: Set<string>;
  undoStack: UndoEntry[];
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
  const [undoStack, setUndoStack] = useState<UndoEntry[]>([]);
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

  // ── Undo stack helpers ─────────────────────────────────────────────────────

  function pushUndo(label: string, record: UndoRecord) {
    setUndoStack((prev) => {
      const entry: UndoEntry = {
        id: `${Date.now()}-${Math.random()}`,
        label,
        record,
        timestamp: Date.now(),
      };
      return [entry, ...prev].slice(0, MAX_UNDO);
    });
  }

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
        pushUndo(`重命名为 ${newName}`, result.undo);
        pushToast({
          kind: "success",
          message: `已重命名为 "${newName}"`,
          undoLabel: "撤销",
          onUndo: () => void undoLast(),
        });
      } catch (err) {
        pushToast({ kind: "error", message: `重命名失败：${errorMessage(err)}` });
      }
    },
    [scanId, onRefresh, pushToast],
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
          const record: UndoRecord = {
            kind: "move",
            from: result.succeeded,
            to: result.succeeded.map((p) => {
              const name = p.split("/").pop() ?? p;
              return `${destDir}/${name}`;
            }),
          };
          pushUndo(`移动 ${n} 个文件`, record);
          pushToast({
            kind: "success",
            message: `已移动 ${n} 个文件`,
            undoLabel: "撤销",
            onUndo: () => void undoLast(),
          });
        }
        for (const f of result.failed) {
          pushToast({ kind: "error", message: `移动失败：${f.reason}` });
        }
      } catch (err) {
        pushToast({ kind: "error", message: errorMessage(err) });
      }
    },
    [scanId, onRefresh, clearChecked, pushToast],
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

  const undoLast = useCallback(async () => {
    const [entry, ...rest] = undoStack;
    if (!entry) return;
    setUndoStack(rest);

    const { record } = entry;
    try {
      if (record.kind === "rename" && record.from[0] && record.to[0]) {
        // Undo rename: rename back from `to` → original `from` name
        const originalName = record.from[0].split("/").pop() ?? record.from[0];
        await renameFile(scanId, record.to[0], originalName);
      } else if (record.kind === "move" && record.to.length > 0) {
        // Undo move: move files back to their original directories
        for (let i = 0; i < record.to.length; i++) {
          const origDir = record.from[i]
            ? record.from[i].split("/").slice(0, -1).join("/")
            : "";
          if (origDir) await moveFiles(scanId, [record.to[i]], origDir);
        }
      }
      onRefresh();
      pushToast({ kind: "info", message: `已撤销：${entry.label}` });
    } catch (err) {
      pushToast({ kind: "error", message: `撤销失败：${errorMessage(err)}` });
    }
  }, [undoStack, scanId, onRefresh, pushToast]);

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
    canUndo: undoStack.length > 0,
  };
}
