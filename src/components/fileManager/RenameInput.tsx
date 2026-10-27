// Inline rename input — replaces the file name cell when editing is active.

import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, X } from "lucide-react";

type Props = {
  initialName: string;
  onConfirm: (newName: string) => void;
  onCancel: () => void;
};

export function RenameInput({ initialName, onConfirm, onCancel }: Props) {
  const { t } = useTranslation();
  const [value, setValue] = useState(initialName);
  const inputRef = useRef<HTMLInputElement>(null);
  const finished = useRef(false);

  useEffect(() => {
    const el = inputRef.current;
    if (!el) return;
    el.focus();
    // Select filename without extension so user can retype just the base
    const dot = initialName.lastIndexOf(".");
    el.setSelectionRange(0, dot > 0 ? dot : initialName.length);
  }, [initialName]);

  function confirm() {
    if (finished.current) return;
    finished.current = true;
    const trimmed = value.trim();
    if (trimmed && trimmed !== initialName) onConfirm(trimmed);
    else onCancel();
  }

  function cancel() {
    if (finished.current) return;
    finished.current = true;
    onCancel();
  }

  function handleKeyDown(e: React.KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Enter") {
      e.preventDefault();
      confirm();
    }
    if (e.key === "Escape") {
      e.preventDefault();
      cancel();
    }
  }

  return (
    <span className="fm-rename-editor"
      onClick={event => event.stopPropagation()}
      onDoubleClick={event => event.stopPropagation()}
      onBlur={event => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) cancel();
      }}>
    <input
      ref={inputRef}
      className="fm-rename-input"
      value={value}
      onChange={(e) => setValue(e.target.value)}
      onKeyDown={handleKeyDown}
      title={t("fileManager.renameHint")}
      aria-label={t("fileManager.renameAria")}
    />
    <button type="button" className="icon-button" aria-label={t("fileManager.renameSave")}
      onPointerDown={event => event.preventDefault()} onClick={confirm}><Check size={13} /></button>
    <button type="button" className="icon-button" aria-label={t("fileManager.renameCancel")}
      onPointerDown={event => event.preventDefault()} onClick={cancel}><X size={13} /></button>
    </span>
  );
}
