// Toast notification for operation feedback and undo prompts.
// Self-dismisses after `duration` ms; undo button resets the timer.

import { useEffect, useRef, useState } from "react";
import { CheckCircle, AlertTriangle, X, RotateCcw } from "lucide-react";

export type ToastKind = "success" | "error" | "info";

export type ToastData = {
  id: string;
  kind: ToastKind;
  message: string;
  undoLabel?: string;
  onUndo?: () => void;
  duration?: number; // ms, default 4000
};

type Props = {
  toasts: ToastData[];
  onDismiss: (id: string) => void;
};

function ToastItem({
  toast,
  onDismiss,
}: {
  toast: ToastData;
  onDismiss: (id: string) => void;
}) {
  const duration = toast.duration ?? 4000;
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  function resetTimer() {
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = setTimeout(() => onDismiss(toast.id), duration);
  }

  useEffect(() => {
    resetTimer();
    return () => { if (timerRef.current) clearTimeout(timerRef.current); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function handleUndo() {
    toast.onUndo?.();
    onDismiss(toast.id);
  }

  const Icon =
    toast.kind === "success" ? CheckCircle
    : toast.kind === "error"  ? AlertTriangle
    : CheckCircle;

  return (
    <div className={`fm-toast fm-toast--${toast.kind}`} role="status" aria-live="polite">
      <Icon size={14} className="fm-toast-icon" />
      <span className="fm-toast-msg">{toast.message}</span>
      {toast.onUndo && (
        <button
          type="button"
          className="fm-toast-undo"
          onClick={handleUndo}
          onMouseEnter={resetTimer}
        >
          <RotateCcw size={12} />
          {toast.undoLabel ?? "撤销"}
        </button>
      )}
      <button
        type="button"
        className="fm-toast-close icon-button"
        onClick={() => onDismiss(toast.id)}
        aria-label="关闭"
      >
        <X size={12} />
      </button>
    </div>
  );
}

export function ToastStack({ toasts, onDismiss }: Props) {
  if (toasts.length === 0) return null;
  return (
    <div className="fm-toast-stack" aria-label="操作通知">
      {toasts.map((t) => (
        <ToastItem key={t.id} toast={t} onDismiss={onDismiss} />
      ))}
    </div>
  );
}

// ── useToasts hook ─────────────────────────────────────────────────────────────

export function useToasts() {
  const [toasts, setToasts] = useState<ToastData[]>([]);

  function push(toast: Omit<ToastData, "id">) {
    const id = `${Date.now()}-${Math.random()}`;
    setToasts((prev) => [...prev, { ...toast, id }]);
  }

  function dismiss(id: string) {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }

  return { toasts, push, dismiss };
}
