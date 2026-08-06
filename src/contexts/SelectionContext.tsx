import { createContext, useContext, useState, ReactNode } from "react";

export type SelectionMode = "none" | "page" | "all";

type SelectionKey = string; // Format: "scanId:fileId"

interface SelectionContextValue {
  mode: SelectionMode;
  selected: Set<SelectionKey>;
  scanId: string | null;

  // Single item operations
  isSelected: (scanId: string, fileId: number) => boolean;
  toggle: (scanId: string, fileId: number) => void;

  // Batch operations
  selectPage: (scanId: string, fileIds: number[]) => void;
  deselectPage: (scanId: string, fileIds: number[]) => void;
  selectMultiple: (scanId: string, fileIds: number[]) => void;
  deselectMultiple: (scanId: string, fileIds: number[]) => void;
  selectAll: (scanId: string) => void;

  // Clear
  clearSelection: () => void;
  clear: () => void; // Alias for clearSelection

  // Metadata
  count: number;
}

const SelectionContext = createContext<SelectionContextValue | undefined>(
  undefined
);

export function SelectionProvider({ children }: { children: ReactNode }) {
  const [mode, setMode] = useState<SelectionMode>("none");
  const [selected, setSelected] = useState<Set<SelectionKey>>(new Set());
  const [scanId, setScanId] = useState<string | null>(null);

  const makeKey = (scanId: string, fileId: number): SelectionKey =>
    `${scanId}:${fileId}`;

  const isSelected = (scanId: string, fileId: number): boolean =>
    selected.has(makeKey(scanId, fileId));

  const toggle = (scanId: string, fileId: number) => {
    const key = makeKey(scanId, fileId);
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(key)) {
        next.delete(key);
        if (next.size === 0) {
          setMode("none");
          setScanId(null);
        }
      } else {
        next.add(key);
        if (mode === "none") {
          setMode("page");
          setScanId(scanId);
        }
      }
      return next;
    });
  };

  const selectPage = (scanId: string, fileIds: number[]) => {
    setSelected((prev) => {
      const next = new Set(prev);
      fileIds.forEach((id) => next.add(makeKey(scanId, id)));
      return next;
    });
    if (mode === "none") {
      setMode("page");
      setScanId(scanId);
    }
  };

  const deselectPage = (scanId: string, fileIds: number[]) => {
    setSelected((prev) => {
      const next = new Set(prev);
      fileIds.forEach((id) => next.delete(makeKey(scanId, id)));
      if (next.size === 0) {
        setMode("none");
        setScanId(null);
      }
      return next;
    });
  };

  // Aliases for consistency
  const selectMultiple = selectPage;
  const deselectMultiple = deselectPage;

  const selectAll = (scanId: string) => {
    setMode("all");
    setScanId(scanId);
    // In "all" mode, we don't enumerate every ID — backend will handle bulk ops
    setSelected(new Set());
  };

  const clearSelection = () => {
    setMode("none");
    setSelected(new Set());
    setScanId(null);
  };

  return (
    <SelectionContext.Provider
      value={{
        mode,
        selected,
        scanId,
        isSelected,
        toggle,
        selectPage,
        deselectPage,
        selectMultiple,
        deselectMultiple,
        selectAll,
        clearSelection,
        clear: clearSelection,
        count: mode === "all" ? Infinity : selected.size,
      }}
    >
      {children}
    </SelectionContext.Provider>
  );
}

export function useSelection() {
  const context = useContext(SelectionContext);
  if (!context) {
    throw new Error("useSelection must be used within SelectionProvider");
  }
  return context;
}
