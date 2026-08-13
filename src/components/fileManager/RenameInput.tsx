// Inline rename input — replaces the file name cell when editing is active.

import { useEffect, useRef, useState } from "react";

type Props = {
  initialName: string;
  onConfirm: (newName: string) => void;
  onCancel: () => void;
};

export function RenameInput({ initialName, onConfirm, onCancel }: Props) {
  const [value, setValue] = useState(initialName);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const el = inputRef.current;
    if (!el) return;
    el.focus();
    // Select filename without extension so user can retype just the base
    const dot = initialName.lastIndexOf(".");
    el.setSelectionRange(0, dot > 0 ? dot : initialName.length);
  }, [initialName]);

  function handleKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      const trimmed = value.trim();
      if (trimmed && trimmed !== initialName) onConfirm(trimmed);
      else onCancel();
    }
    if (e.key === "Escape") {
      e.stopPropagation();
      onCancel();
    }
  }

  return (
    <input
      ref={inputRef}
      className="fm-rename-input"
      value={value}
      onChange={(e) => setValue(e.target.value)}
      onKeyDown={handleKeyDown}
      onBlur={() => {
        const trimmed = value.trim();
        if (trimmed && trimmed !== initialName) onConfirm(trimmed);
        else onCancel();
      }}
      // Prevent clicks inside the input from propagating to the row
      onClick={(e) => e.stopPropagation()}
      aria-label="重命名文件"
    />
  );
}
