import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronRight, Trash2 } from "lucide-react";
import { trashFiles } from "../lib/tauri";
import { appStore } from "../app/store";
import { useToast } from "../contexts/ToastContext";
import { useAsyncAction } from "../hooks/useAsyncAction";
import { categoryColor, categoryIcon, categoryTint } from "../lib/categories";
import { formatBytes, formatDate } from "../lib/format";
import { errorMessage } from "../lib/errors";
import { ConfirmDialog } from "./fileManager/ConfirmDialog";
import type { FileEntry } from "../types/scan";

export function LargeFiles({
  files,
  onReveal,
  scanId,
}: {
  files: FileEntry[];
  onReveal: (path: string) => void;
  scanId?: string;
}) {
  const { t } = useTranslation();
  const toast = useToast();
  const trashAction = useAsyncAction<"trash">();
  const [trashPending, setTrashPending] = useState<FileEntry | null>(null);

  async function confirmTrash() {
    const file = trashPending;
    if (!file || !scanId) return;
    setTrashPending(null);
    await trashAction.run("trash", async () => {
      try {
        const result = await trashFiles(scanId, [file.path]);
        appStore.bumpDataRevision();
        if (result.succeeded.length > 0) {
          toast.push({ kind: "success", message: t("insight.trashDone", { name: file.name }) });
        }
        for (const failure of result.failed) {
          toast.push({ kind: "error", message: t("fileManager.ops.trashItemFailed", { reason: failure.reason }) });
        }
      } catch (error) {
        toast.push({ kind: "error", message: errorMessage(error) });
      }
    });
  }

  return (
    <section className="result-section" aria-labelledby="large-files-title">
      <div className="section-heading compact-heading">
        <h2 id="large-files-title">{t("largeFiles.title")}</h2>
        <span>{t("largeFiles.topN", { count: files.length })}</span>
      </div>
      {files.length ? (
        <div className="file-list">
          {files.map((file) => {
            const Icon = categoryIcon(file.category);
            const tint = categoryColor(file.category);
            return (
              <div className="file-row" key={file.path}>
                <span
                  className="file-icon"
                  style={{ color: tint, background: categoryTint(file.category) }}
                >
                  <Icon size={16} />
                </span>
                <div className="file-copy">
                  <strong title={file.name}>{file.name}</strong>
                  <span title={file.path}>{file.path}</span>
                </div>
                <div className="file-meta">
                  <strong>{formatBytes(file.sizeBytes)}</strong>
                  <span>{formatDate(file.modifiedAt)}</span>
                </div>
                <span className="file-actions">
                  {scanId && (
                    <button
                      className="icon-button"
                      type="button"
                      title={t("fileManager.list.trashTitle")}
                      aria-label={t("fileManager.list.trashAria", { name: file.name })}
                      disabled={trashAction.pending !== null}
                      onClick={() => setTrashPending(file)}
                    >
                      <Trash2 size={14} />
                    </button>
                  )}
                  <button
                    className="icon-button"
                    type="button"
                    title={t("common.reveal")}
                    aria-label={t("common.revealNamed", { name: file.name })}
                    onClick={() => onReveal(file.path)}
                  >
                    <ChevronRight size={16} />
                  </button>
                </span>
              </div>
            );
          })}
        </div>
      ) : (
        <p className="empty-inline">{t("largeFiles.empty")}</p>
      )}

      <ConfirmDialog
        open={trashPending !== null}
        title={t("fileManager.confirmTrash.title")}
        confirmLabel={t("fileManager.confirmTrash.title")}
        variant="danger"
        message={trashPending
          ? t("fileManager.confirmTrash.messageOne", { name: trashPending.name })
          : ""}
        onConfirm={() => void confirmTrash()}
        onCancel={() => setTrashPending(null)}
      />
    </section>
  );
}
