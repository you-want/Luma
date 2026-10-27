import { Database, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useAsyncAction } from "../hooks/useAsyncAction";
import { clearScanIndex, compactIndexStorage, getIndexStorage, getLatestScan, type IndexStorageUsage } from "../lib/tauri";
import { formatBytes, formatNumber } from "../lib/format";
import { errorMessage } from "../lib/errors";
import { ButtonSpinner } from "./ButtonSpinner";

export function IndexStorage({ disabled, onCleared, onWorkingChange }: {
  disabled: boolean;
  onCleared: () => void;
  onWorkingChange: (working: boolean) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [usage, setUsage] = useState<IndexStorageUsage | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const dialog = useRef<HTMLDialogElement>(null);
  const closeButton = useRef<HTMLButtonElement>(null);
  const action = useAsyncAction<"load" | "compact" | "clear">();
  const busy = action.pending !== null;
  useEffect(() => { onWorkingChange(busy); }, [busy, onWorkingChange]);
  useEffect(() => {
    if (!open) return;
    const previous = document.activeElement as HTMLElement | null;
    dialog.current?.showModal();
    closeButton.current?.focus();
    return () => { dialog.current?.close(); previous?.focus(); };
  }, [open]);

  useEffect(() => {
    if (!open || busy) return;
    if (confirming) {
      dialog.current?.querySelector<HTMLButtonElement>("footer button")?.focus();
      dialog.current?.querySelector(".index-storage-confirm")?.scrollIntoView({ block: "nearest" });
    }
    else if (!dialog.current?.contains(document.activeElement)) closeButton.current?.focus();
  }, [open, busy, confirming]);

  async function load() {
    setOpen(true); setConfirming(false); setError(null); setNotice(null); setUsage(null);
    await action.run("load", async () => {
      try { setUsage(await getIndexStorage()); }
      catch (e) { setError(errorMessage(e)); }
    });
  }
  async function maintain(kind: "compact" | "clear") {
    await action.run(kind, async () => {
      setError(null); setNotice(null);
      try {
        const result = await (kind === "clear" ? clearScanIndex() : compactIndexStorage());
        setUsage(result); setConfirming(false);
        if (result.maintenanceError) setError(t("indexStorage.compactFailed"));
        setNotice(t(kind === "clear" ? "indexStorage.cleared" : "indexStorage.compacted"));
        if (kind === "clear") onCleared();
      } catch (e) {
        setError(errorMessage(e));
        // A compaction failure after deletion must not leave deleted results
        // interactive. Reconcile even when the command fails after commit.
        if (kind === "clear") {
          try { if (!await getLatestScan()) onCleared(); } catch { /* Retain the actionable error. */ }
        }
      }
    });
  }
  function close() { if (!busy) setOpen(false); }

  return <>
    <button type="button" className="index-storage-trigger" disabled={disabled}
      onClick={() => void load()} title={t("indexStorage.title")} aria-label={t("indexStorage.title")}>
      <Database size={14} /><span>{t("indexStorage.shortTitle")}</span>
    </button>
    {open && <dialog ref={dialog} className="index-storage-dialog" aria-labelledby="index-storage-title"
      aria-busy={busy} onCancel={event => { event.preventDefault(); close(); }}
      onKeyDown={event => {
        if (event.key !== "Tab") return;
        const buttons = [...(dialog.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [])];
        if (!buttons.length) { event.preventDefault(); return; }
        event.preventDefault();
        const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
        buttons[(index + (event.shiftKey ? -1 : 1) + buttons.length) % buttons.length]?.focus();
      }}>
      <header><h2 id="index-storage-title">{t("indexStorage.title")}</h2>
        <button ref={closeButton} type="button" className="icon-button" disabled={busy} onClick={close} aria-label={t("fileManager.dialog.close")}><X size={18} /></button>
      </header>
      <div className="index-storage-body">
      <p className="index-storage-description">{t("indexStorage.description")}</p>
      {action.pending === "load" && <p role="status"><ButtonSpinner /> {t("indexStorage.loading")}</p>}
      {usage && <>
        <div className="index-storage-total"><strong>{formatBytes(usage.databaseBytes + usage.journalBytes)}</strong><span>{t("indexStorage.total")}</span></div>
        <dl className="index-storage-stats">
          <div><dt>{t("indexStorage.files")}</dt><dd>{formatNumber(usage.indexedFiles)}</dd></div>
          <div><dt>{t("indexStorage.scans")}</dt><dd>{formatNumber(usage.scanCount)}</dd></div>
          <div><dt>{t("indexStorage.undo")}</dt><dd>{formatNumber(usage.undoRecords)}</dd></div>
          <div><dt>{t("indexStorage.reclaimable")}</dt><dd>{formatBytes(usage.reclaimableBytes)}</dd></div>
        </dl>
        <p className="index-storage-policy">{t("indexStorage.policy", { budget: formatBytes(usage.scanBudgetBytes) })}</p>
        {confirming && <div className="index-storage-confirm">
          <h3>{t("indexStorage.confirmTitle")}</h3><p>{t("indexStorage.confirmBody")}</p>
        </div>}
      </>}
      {notice && <p role="status" className="index-storage-notice">{notice}</p>}
      {error && <p role="alert" className="index-storage-error">{error}</p>}
      </div>
      <footer>
        {!usage && !busy && <button type="button" className="button" onClick={() => void load()}>{t("indexStorage.retry")}</button>}
        {usage && !confirming && <>
          <button type="button" className="button" disabled={busy} onClick={() => void maintain("compact")} aria-busy={action.pending === "compact"}>
            {action.pending === "compact" && <ButtonSpinner />}{t("indexStorage.compact")}
          </button>
          <button type="button" className="button button-danger" disabled={busy || usage.scanCount === 0} onClick={() => setConfirming(true)}>{t("indexStorage.clear")}</button>
        </>}
        {confirming && <>
          <button type="button" className="button" disabled={busy} onClick={() => setConfirming(false)}>{t("fileManager.dialog.cancel")}</button>
          <button type="button" className="button button-danger" disabled={busy} onClick={() => void maintain("clear")} aria-busy={action.pending === "clear"}>
            {action.pending === "clear" && <ButtonSpinner />}{t("indexStorage.confirmClear")}
          </button>
        </>}
      </footer>
    </dialog>}
  </>;
}
