// Confirmation dialog for destructive actions (trash, move-overwrite, etc.)
// Fully controlled — caller owns open/close state.

import { AlertTriangle, Trash2 } from "lucide-react";
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";

type Variant = "danger" | "warning";

type Props = {
  open: boolean;
  title: string;
  message: string;
  confirmLabel?: string;
  cancelLabel?: string;
  variant?: Variant;
  onConfirm: () => void;
  onCancel: () => void;
};

export function ConfirmDialog({
  open,
  title,
  message,
  confirmLabel,
  cancelLabel,
  variant = "danger",
  onConfirm,
  onCancel,
}: Props) {
  const { t } = useTranslation();
  const cancelRef = useRef<HTMLButtonElement>(null);
  const confirmRef = useRef<HTMLButtonElement>(null);
  const dialogRef = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    if (!open) return;
    const previous = document.activeElement as HTMLElement | null;
    dialogRef.current?.showModal();
    cancelRef.current?.focus();
    return () => { dialogRef.current?.close(); previous?.focus(); };
  }, [open]);

  if (!open) return null;

  return (
    <dialog ref={dialogRef} className="fm-dialog"
      aria-labelledby="fm-dialog-title" aria-describedby="fm-dialog-msg"
      onCancel={event => { event.preventDefault(); onCancel(); }}
      onKeyDown={event => {
        // WebKit follows the system keyboard-navigation preference, which may
        // skip buttons. Keep both choices reachable inside this modal.
        if (event.key !== "Tab") return;
        event.preventDefault();
        const buttons = [cancelRef.current, confirmRef.current];
        const current = buttons.indexOf(document.activeElement as HTMLButtonElement);
        buttons[(current + (event.shiftKey ? -1 : 1) + buttons.length) % buttons.length]?.focus();
      }}>
        <div className="fm-dialog-icon" data-variant={variant}>
          {variant === "danger" ? <Trash2 size={20} /> : <AlertTriangle size={20} />}
        </div>
        <h2 id="fm-dialog-title" className="fm-dialog-title">{title}</h2>
        <p id="fm-dialog-msg" className="fm-dialog-msg">{message}</p>
        <div className="fm-dialog-actions">
          <button ref={cancelRef} type="button" className="button" onClick={onCancel}>
            {cancelLabel ?? t("fileManager.dialog.cancel")}
          </button>
          <button
            ref={confirmRef}
            type="button"
            className={`button ${variant === "danger" ? "button-danger" : "button-primary"}`}
            onClick={onConfirm}
          >
            {confirmLabel ?? t("fileManager.dialog.confirm")}
          </button>
        </div>
    </dialog>
  );
}
