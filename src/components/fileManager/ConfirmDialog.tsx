// Confirmation dialog for destructive actions (trash, move-overwrite, etc.)
// Fully controlled — caller owns open/close state.

import { AlertTriangle, Trash2 } from "lucide-react";
import { useEffect, useRef } from "react";

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
  confirmLabel = "确认",
  cancelLabel = "取消",
  variant = "danger",
  onConfirm,
  onCancel,
}: Props) {
  const confirmRef = useRef<HTMLButtonElement>(null);

  // Auto-focus confirm button; trap Escape to cancel
  useEffect(() => {
    if (!open) return;
    confirmRef.current?.focus();
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onCancel();
    }
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onCancel]);

  if (!open) return null;

  return (
    <div
      className="fm-dialog-backdrop"
      role="presentation"
      onClick={(e) => { if (e.target === e.currentTarget) onCancel(); }}
    >
      <dialog
        className="fm-dialog"
        open
        aria-modal
        aria-labelledby="fm-dialog-title"
        aria-describedby="fm-dialog-msg"
      >
        <div className="fm-dialog-icon" data-variant={variant}>
          {variant === "danger" ? <Trash2 size={20} /> : <AlertTriangle size={20} />}
        </div>
        <h2 id="fm-dialog-title" className="fm-dialog-title">{title}</h2>
        <p id="fm-dialog-msg" className="fm-dialog-msg">{message}</p>
        <div className="fm-dialog-actions">
          <button type="button" className="button" onClick={onCancel}>
            {cancelLabel}
          </button>
          <button
            ref={confirmRef}
            type="button"
            className={`button ${variant === "danger" ? "button-danger" : "button-primary"}`}
            onClick={onConfirm}
          >
            {confirmLabel}
          </button>
        </div>
      </dialog>
    </div>
  );
}
