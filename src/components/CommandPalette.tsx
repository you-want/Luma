import { useState, useEffect, useRef, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { Search, Zap, Layers, ArchiveRestore, FileText } from "lucide-react";
import type { FileEntry } from "../types/scan";
import { searchFiles, revealPath } from "../lib/tauri";
import { formatBytes } from "../lib/format";
import { errorMessage } from "../lib/errors";

type Action = {
  kind: "action";
  id: string;
  label: string;
  kw: string[];
  icon: React.ReactNode;
  onSelect: () => void;
};
type FileItem = { kind: "file"; entry: FileEntry };
type Item = Action | FileItem;

type Props = { scanId: string | null; onNewScan: () => void };

export default function CommandPalette({ scanId, onNewScan }: Props) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [idx, setIdx] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const close = useCallback(() => {
    setOpen(false);
    setQuery("");
    setFiles([]);
    setIdx(0);
  }, []);

  const goto = (sel: string) =>
    document.querySelector(sel)?.scrollIntoView({ behavior: "smooth", block: "start" });

  const buildActions = useCallback((): Action[] => {
    const base: Action[] = [{
      kind: "action", id: "new",
      label: t("palette.action.newScan"),
      kw: ["scan", "扫描", "new", "新建", "开始"],
      icon: <Zap size={14} />,
      onSelect: () => { close(); onNewScan(); },
    }];
    if (!scanId) return base;
    return [...base,
      { kind: "action", id: "search", label: t("palette.action.goSearch"), kw: ["search", "搜索", "find", "查找"], icon: <Search size={14} />, onSelect: () => { close(); goto(".search-panel"); } },
      { kind: "action", id: "dup", label: t("palette.action.goDuplicates"), kw: ["dup", "重复", "duplicate", "相同"], icon: <Layers size={14} />, onSelect: () => { close(); goto(".duplicates-section"); } },
      { kind: "action", id: "clean", label: t("palette.action.goCleanup"), kw: ["clean", "清理", "cleanup", "释放"], icon: <ArchiveRestore size={14} />, onSelect: () => { close(); goto(".cleanup-section"); } },
      { kind: "action", id: "large", label: t("palette.action.goLargeFiles"), kw: ["large", "大文件", "biggest", "最大"], icon: <FileText size={14} />, onSelect: () => { close(); goto(".large-files-section"); } },
    ];
  }, [t, scanId, close, onNewScan]);

  // ⌘K / Ctrl+K global shortcut
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "k") { e.preventDefault(); setOpen(p => !p); }
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, []);

  useEffect(() => {
    if (open) requestAnimationFrame(() => inputRef.current?.focus());
  }, [open]);

  // Debounced file search
  useEffect(() => {
    if (timer.current) clearTimeout(timer.current);
    if (!scanId || !query) { setFiles([]); setLoading(false); return; }
    setLoading(true);
    timer.current = setTimeout(async () => {
      try {
        const r = await searchFiles({ scanId, query: query.trim(), limit: 6, offset: 0, sort: "sizeDesc", includeHidden: false });
        setFiles(r.files);
      } catch { setFiles([]); }
      finally { setLoading(false); }
    }, 150);
    return () => { if (timer.current) clearTimeout(timer.current); };
  }, [query, scanId]);

  const q = query.toLowerCase();
  const acts = buildActions().filter(a => !q || a.label.toLowerCase().includes(q) || a.kw.some(k => k.includes(q)));
  const items: Item[] = [...acts, ...files.map((f): FileItem => ({ kind: "file", entry: f }))];

  useEffect(() => setIdx(0), [query]);
  useEffect(() => {
    (listRef.current?.children[idx] as HTMLElement | undefined)?.scrollIntoView({ block: "nearest" });
  }, [idx]);

  const pick = useCallback(async (file: FileEntry) => {
    try { await revealPath(file.path); } catch (e) { console.error(errorMessage(e)); }
    close();
  }, [close]);

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") { close(); return; }
    if (e.key === "ArrowDown") { e.preventDefault(); setIdx(i => Math.min(i + 1, items.length - 1)); }
    if (e.key === "ArrowUp") { e.preventDefault(); setIdx(i => Math.max(i - 1, 0)); }
    if (e.key === "Enter") {
      e.preventDefault();
      const it = items[idx];
      if (!it) return;
      it.kind === "action" ? it.onSelect() : void pick(it.entry);
    }
  };

  if (!open) return null;

  return (
    <div className="palette-backdrop" onClick={close} role="presentation">
      <div className="palette-panel" onClick={e => e.stopPropagation()} onKeyDown={onKey}
        role="dialog" aria-modal aria-label={t("palette.ariaLabel")}>

        <div className="palette-input-row">
          <Search size={15} className="palette-search-icon" aria-hidden />
          <input ref={inputRef} className="palette-input" placeholder={t("palette.placeholder")}
            value={query} onChange={e => setQuery(e.target.value)} autoComplete="off" spellCheck={false} />
          {loading && <span className="palette-spinner" aria-hidden />}
        </div>

        {items.length > 0 && (
          <ul className="palette-list" ref={listRef} role="listbox">
            {acts.length > 0 && <>
              <li className="palette-group-label" role="presentation">{t("palette.group.actions")}</li>
              {acts.map((a, i) => (
                <li key={a.id} className={`palette-item${idx === i ? " palette-item--active" : ""}`}
                  role="option" aria-selected={idx === i} onMouseEnter={() => setIdx(i)} onClick={a.onSelect}>
                  <span className="palette-item-icon" aria-hidden>{a.icon}</span>
                  <span className="palette-item-label">{a.label}</span>
                  <kbd className="palette-kbd" aria-hidden>↵</kbd>
                </li>
              ))}
            </>}
            {files.length > 0 && <>
              <li className="palette-group-label" role="presentation">{t("palette.group.files")}</li>
              {files.map((f, i) => (
                <li key={f.path} className={`palette-item${idx === acts.length + i ? " palette-item--active" : ""}`}
                  role="option" aria-selected={idx === acts.length + i}
                  onMouseEnter={() => setIdx(acts.length + i)} onClick={() => void pick(f)}>
                  <span className="palette-item-icon palette-item-icon--muted" aria-hidden><FileText size={13} /></span>
                  <span className="palette-item-file">
                    <span className="palette-item-name">{f.name}</span>
                    <span className="palette-item-meta">{formatBytes(f.sizeBytes)} · {f.path}</span>
                  </span>
                  <span className="palette-item-reveal" aria-hidden>{t("palette.reveal")}</span>
                </li>
              ))}
            </>}
          </ul>
        )}

        {!loading && query.length > 0 && items.length === 0 && (
          <p className="palette-empty">{t("palette.noResults")}</p>
        )}

        <div className="palette-footer" aria-hidden>
          <span><kbd>↑↓</kbd>{t("palette.hint.navigate")}</span>
          <span><kbd>↵</kbd>{t("palette.hint.select")}</span>
          <span><kbd>Esc</kbd>{t("palette.hint.close")}</span>
        </div>
      </div>
    </div>
  );
}
