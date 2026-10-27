import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { AlertTriangle, Calendar, CheckCircle, ChevronLeft, FolderOpen, Layers, Sparkles, Tag, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { chooseDirectory, executeOrganizePlan, planOrganize } from "../../lib/tauri";
import { errorMessage } from "../../lib/errors";
import { useAsyncAction } from "../../hooks/useAsyncAction";
import { ButtonSpinner } from "../ButtonSpinner";
import type { OrganizeMove, OrganizeOptions, OrganizePlan, OrganizeProgress, OrganizeResult, OrganizeRule } from "../../types/fileManager";

type Props = {
  open: boolean;
  scanId: string;
  defaultSourceDir: string;
  onClose: () => void;
  onDone: (operationId?: string) => void;
};
type WizardStep = "rule" | "dirs" | "preview" | "executing";
const STEPS: WizardStep[] = ["rule", "dirs", "preview", "executing"];
const RULES = [
  { kind: "byPurpose", Icon: Sparkles },
  { kind: "byCategory", Icon: Layers },
  { kind: "byYear", Icon: Calendar },
  { kind: "byYearMonth", Icon: Calendar },
  { kind: "byExtension", Icon: Tag },
] as const;
const initialOptions = (language: string): OrganizeOptions => ({
  recursive: false, conflictStrategy: "skip", language,
  folderNames: {}, fileFolders: {}, keywordRules: [],
});

function StepExecuting({
  progress,
  result,
  error,
}: {
  progress: OrganizeProgress | null;
  result: { succeeded: number; failed: { path: string; reason: string }[] } | null;
  error: string | null;
}) {
  const { t } = useTranslation();
  if (error) {
    return (
      <div className="org-step org-exec-error">
        <AlertTriangle size={24} />
        <p>{error}</p>
      </div>
    );
  }

  if (result) {
    return (
      <div className="org-step org-exec-done">
        <CheckCircle size={32} className="org-exec-check" />
        <strong>{t("fileManager.organize.done.title")}</strong>
        <p>{t("fileManager.organize.done.moved", { count: result.succeeded })}</p>
        {result.failed.length > 0 && (
          <div className="org-exec-failures">
            <p className="org-exec-fail-title">{t("fileManager.organize.done.failedTitle", { count: result.failed.length })}</p>
            {result.failed.map((f) => (
              <div key={f.path} className="org-exec-fail-row">
                <span title={f.path}>{f.path.split("/").pop()}</span>
                <span className="org-exec-fail-reason">{f.reason}</span>
              </div>
            ))}
          </div>
        )}
      </div>
    );
  }

  const pct = progress && progress.total > 0
    ? Math.round((progress.done / progress.total) * 100)
    : 0;
  const currentName = progress?.currentFrom?.split("/").pop() ?? "";

  return (
    <div className="org-step org-exec-progress">
      <div className="org-exec-bar-wrap">
        <div className="org-exec-bar" style={{ width: `${pct}%` }} />
      </div>
      <p className="org-exec-pct">{pct}%</p>
      {progress && (
        <p className="org-exec-current" title={progress.currentFrom}>
          {progress.done} / {progress.total}
          {currentName && <> — {currentName}</>}
        </p>
      )}
    </div>
  );
}


// Pagination is per group; selecting a group always applies to all its files.
function PreviewGroup({ moves, folderName, options, excluded, busy, onRename, onOptions, onExcluded }: {
  moves: OrganizeMove[];
  folderName: string;
  options: OrganizeOptions;
  excluded: Set<string>;
  busy: boolean;
  onRename: (name: string) => void;
  onOptions: (next: OrganizeOptions) => void;
  onExcluded: (paths: string[], exclude: boolean) => void;
}) {
  const { t } = useTranslation();
  const [page, setPage] = useState(0);
  const checkbox = useRef<HTMLInputElement>(null);
  const eligible = moves.filter(m => !m.conflict);
  const selected = eligible.filter(m => !excluded.has(m.from)).length;
  useEffect(() => {
    if (checkbox.current) checkbox.current.indeterminate = selected > 0 && selected < eligible.length;
  }, [selected, eligible.length]);
  const first = moves[0];
  const pages = Math.max(1, Math.ceil(moves.length / 50));
  const currentPage = Math.min(page, pages - 1);
  return <section className="org-folder-group">
    <div className="org-folder-heading">
      <input ref={checkbox} type="checkbox" checked={selected > 0 && selected === eligible.length}
        disabled={busy || eligible.length === 0}
        aria-label={t("fileManager.organize.selectGroup", { folder: first.subfolder })}
        onChange={e => onExcluded(eligible.map(m => m.from), !e.target.checked)} />
      <FolderOpen size={17} aria-hidden="true" />
      <input className="org-folder-input" aria-label={t("fileManager.organize.folderName", { folder: first.subfolder })}
        value={folderName} disabled={busy}
        onChange={e => onRename(e.target.value)} />
      <span className="org-folder-count">{t("fileManager.organize.groupCount", { selected, count: moves.length })}</span>
    </div>
    {moves.slice(currentPage * 50, (currentPage + 1) * 50).map(m => <div key={m.from}
      className={`org-file-row${m.conflict || excluded.has(m.from) ? " org-file-row--skipped" : ""}`}>
      <input type="checkbox" checked={!m.conflict && !excluded.has(m.from)} disabled={busy || m.conflict}
        aria-label={t("fileManager.organize.selectFile", { name: m.fromName })}
        onChange={e => onExcluded([m.from], !e.target.checked)} />
      <div className="org-file-info">
        <strong title={m.from}>{m.fromName}</strong>
        <span title={m.from}>{m.from}</span>
        <span title={m.to}>→ {m.to}</span>
        <span>{m.matchedKeyword
          ? t("fileManager.organize.keywordReason", { keyword: m.matchedKeyword })
          : t("fileManager.organize.metadataReason")}</span>
        {m.conflict && <span className="org-preview-conflict">{t("fileManager.organize.conflictHint")}</span>}
        {m.renamed && <span>{t("fileManager.organize.renamed")}</span>}
      </div>
      <label className="org-file-destination">
        <span>{t("fileManager.organize.table.target")}</span>
        <input value={options.fileFolders[m.from] ?? m.subfolder} disabled={busy}
          aria-label={t("fileManager.organize.fileFolder", { name: m.fromName })}
          onChange={e => onOptions({ ...options, fileFolders: { ...options.fileFolders, [m.from]: e.target.value } })} />
      </label>
    </div>)}
    {pages > 1 && <div className="fm-pagination">
      <button type="button" className="button" disabled={busy || currentPage === 0} onClick={() => setPage(currentPage - 1)}>{t("fileManager.pagination.previous")}</button>
      <span>{t("fileManager.pagination.info", { current: currentPage + 1, total: pages })}</span>
      <button type="button" className="button" disabled={busy || currentPage === pages - 1} onClick={() => setPage(currentPage + 1)}>{t("fileManager.pagination.next")}</button>
    </div>}
  </section>;
}

export function OrganizeWizard({ open, scanId, defaultSourceDir, onClose, onDone }: Props) {
  const { t, i18n } = useTranslation();
  const [step, setStep] = useState<WizardStep>("rule");
  const [rule, setRule] = useState<OrganizeRule>({ kind: "byPurpose" });
  const [sourceDir, setSourceDir] = useState(defaultSourceDir);
  const [destDir, setDestDir] = useState(defaultSourceDir);
  const [options, setOptions] = useState(() => initialOptions(i18n.language));
  const [excluded, setExcluded] = useState<Set<string>>(new Set());
  const [folderDrafts, setFolderDrafts] = useState<Record<string, string>>({});
  const [dirty, setDirty] = useState(false);
  const [planning, setPlanning] = useState(false);
  const [plan, setPlan] = useState<OrganizePlan | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  const [progress, setProgress] = useState<OrganizeProgress | null>(null);
  const [result, setResult] = useState<OrganizeResult | null>(null);
  const [execError, setExecError] = useState<string | null>(null);
  const requestId = useRef(0);
  const executing = useRef(false);
  const dialog = useRef<HTMLDialogElement>(null);
  const action = useAsyncAction<"source" | "dest" | "preview">();
  const busy = planning || action.pending !== null;

  useEffect(() => {
    requestId.current += 1;
    if (!open) return;
    setStep("rule");
    setRule({ kind: "byPurpose" });
    setSourceDir(defaultSourceDir);
    setDestDir(defaultSourceDir);
    setOptions(initialOptions(i18n.language));
    setExcluded(new Set());
    setFolderDrafts({});
    setDirty(false);
    setPlanning(false);
    setPlan(null);
    setPlanError(null);
    setProgress(null);
    setResult(null);
    setExecError(null);
    executing.current = false;
    const previous = document.activeElement as HTMLElement | null;
    dialog.current?.showModal();
    dialog.current?.querySelector<HTMLElement>("h2")?.focus();
    return () => {
      requestId.current += 1;
      dialog.current?.close();
      previous?.focus();
    };
    // Language changes apply to the next generated plan without resetting work.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, defaultSourceDir, scanId]);

  const groups = useMemo(() => {
    const map = new Map<string, OrganizeMove[]>();
    for (const move of plan?.moves ?? []) {
      const group = map.get(move.subfolder) ?? [];
      group.push(move);
      map.set(move.subfolder, group);
    }
    return [...map.entries()];
  }, [plan]);
  const selected = plan?.moves.filter(m => !m.conflict && !excluded.has(m.from)) ?? [];
  const isDone = result !== null || execError !== null;
  const canClose = step !== "executing" || isDone;

  useEffect(() => {
    if (!open) return;
    dialog.current?.querySelector<HTMLElement>(".org-wizard-body")?.scrollTo({ top: 0 });
    dialog.current?.querySelector<HTMLElement>("h2")?.focus();
  }, [step, open]);

  if (!open) return null;

  function close() {
    if (!canClose) return;
    if (step === "executing") onDone(result?.operationId);
    onClose();
  }
  function changeOptions(next: OrganizeOptions) {
    setOptions(next);
    setDirty(true);
  }
  function exclude(paths: string[], value: boolean) {
    setExcluded(previous => {
      const next = new Set(previous);
      for (const path of paths) { if (value) next.add(path); else next.delete(path); }
      return next;
    });
  }
  async function pick(which: "source" | "dest") {
    await action.run(which, async () => {
      const id = requestId.current;
      try {
        const path = await chooseDirectory();
        if (!path || id !== requestId.current) return;
        if (which === "source") {
          setSourceDir(path);
          if (destDir === sourceDir) setDestDir(path);
          setOptions(previous => ({ ...previous, folderNames: {}, fileFolders: {} }));
          setExcluded(new Set());
        } else setDestDir(path);
        setDirty(true);
        setPlanError(null);
      } catch (error) {
        if (id === requestId.current) setPlanError(errorMessage(error));
      }
    });
  }
  async function preview() {
    await action.run("preview", async () => {
      const id = ++requestId.current;
      setPlanning(true);
      setPlanError(null);
      try {
        const next = await planOrganize(scanId, sourceDir, destDir || sourceDir, rule, { ...options, language: i18n.language });
        if (id !== requestId.current) return;
        setPlan(next);
        setFolderDrafts({});
        setDirty(false);
        setStep("preview");
      } catch (error) {
        if (id === requestId.current) setPlanError(errorMessage(error));
      } finally {
        if (id === requestId.current) setPlanning(false);
      }
    });
  }
  async function execute() {
    if (executing.current || busy || dirty || selected.length === 0) return;
    executing.current = true;
    setStep("executing");
    let unlisten: (() => void) | undefined;
    try {
      unlisten = await listen<OrganizeProgress>("organize-progress", event => setProgress(event.payload));
      const outcome = await executeOrganizePlan(scanId, selected.map(m => ({
        from: m.from, to: m.to, expectedSizeBytes: m.expectedSizeBytes, expectedModifiedAt: m.expectedModifiedAt,
      })));
      setResult(outcome);
    } catch (error) { setExecError(errorMessage(error)); }
    finally { unlisten?.(); }
  }

  return <dialog ref={dialog} className="org-wizard" data-step={step} aria-labelledby="org-wizard-title"
    onCancel={e => { e.preventDefault(); close(); }}
    onClick={e => { if (e.target === e.currentTarget) {
      const rect = e.currentTarget.getBoundingClientRect();
      if (e.clientX < rect.left || e.clientX > rect.right || e.clientY < rect.top || e.clientY > rect.bottom) close();
    } }}>
    <div className="org-wizard-header">
      <div>
        <p className="org-wizard-eyebrow">{t("fileManager.organize.title")}</p>
        <h2 id="org-wizard-title" className="org-wizard-title" tabIndex={-1}>{t(
          result ? "fileManager.organize.done.title" : execError ? "fileManager.organize.failedTitle" : `fileManager.steps.${step}`,
        )}</h2>
      </div>
      {canClose && <button type="button" className="icon-button org-wizard-close" onClick={close} aria-label={t("fileManager.dialog.close")}><X size={18} /></button>}
    </div>
    <ol className="org-steps-bar" aria-label={t("fileManager.organize.stepsLabel")}>{STEPS.map((s, i) => <li key={s}
      aria-current={s === step ? "step" : undefined}
      className={`org-step-marker${s === step ? " org-step-marker--active" : ""}${STEPS.indexOf(step) > i ? " org-step-marker--done" : ""}`}>
      <span className="org-step-number" aria-hidden="true">{STEPS.indexOf(step) > i ? <CheckCircle size={15} /> : i + 1}</span>
      <span>{t(`fileManager.organize.stepLabels.${s}`)}</span>
    </li>)}</ol>
    <div className="org-wizard-body">
      {step === "rule" && <div className="org-step">
        <p className="org-step-hint">{t("fileManager.organize.ruleHint")}</p>
        <div className="org-rule-grid">{RULES.map(({ kind, Icon }) => <button key={kind} type="button"
          className={`org-rule-card${kind === rule.kind ? " org-rule-card--active" : ""}`}
          aria-pressed={kind === rule.kind}
          onClick={() => { setRule({ kind }); setOptions(previous => ({ ...previous, folderNames: {}, fileFolders: {} })); setDirty(true); }}>
          <span className="org-rule-icon"><Icon size={20} /></span>
          <span className="org-rule-copy">
            <strong>{t(`fileManager.organize.rules.${kind}.label`)}</strong>
            <span>{t(`fileManager.organize.rules.${kind}.description`)}</span>
            <code>{t(`fileManager.organize.rules.${kind}.example`)}</code>
          </span>
          <span className="org-rule-selected" aria-hidden="true">{kind === rule.kind && <CheckCircle size={17} />}</span>
        </button>)}</div>
      </div>}
      {step === "dirs" && <div className="org-step">
        <p className="org-step-hint">{t("fileManager.organize.dirsHint")}</p>
        <fieldset disabled={busy} className="org-settings">
          {(["source", "dest"] as const).map(which => <div key={which} className="org-dir-label">
            <span>{t(which === "source" ? "fileManager.organize.sourceDir" : "fileManager.organize.destDir")}</span>
            <div className="org-dir-row"><span className="org-dir-path" title={which === "source" ? sourceDir : destDir}>
              {(which === "source" ? sourceDir : destDir) || t("fileManager.organize.destSameHint")}</span>
              <button type="button" className="button org-dir-btn" aria-busy={action.pending === which} onClick={() => void pick(which)}>
                {action.pending === which ? <ButtonSpinner /> : <FolderOpen size={14} />}{t("fileManager.organize.choose")}</button>
            </div>
          </div>)}
          <label className="org-check"><input type="checkbox" checked={options.recursive}
            onChange={e => changeOptions({ ...options, recursive: e.target.checked })} />{t("fileManager.organize.recursive")}</label>
          <p className={options.recursive ? "org-recursive-warning" : "org-step-hint"}>{t(options.recursive ? "fileManager.organize.recursiveWarning" : "fileManager.organize.preserveHint")}</p>
          <div className="org-keyword-header"><strong>{t("fileManager.organize.keywordRules")}</strong>
            <button type="button" className="button" onClick={() => changeOptions({ ...options, keywordRules: [...options.keywordRules, { keyword: "", folder: "" }] })}>{t("fileManager.organize.addRule")}</button></div>
          <p className="org-step-hint">{t("fileManager.organize.keywordHint")}</p>
          {options.keywordRules.map((entry, i) => <div className="org-keyword-row" key={i}>
            <input aria-label={t("fileManager.organize.keyword")} placeholder={t("fileManager.organize.keyword")}
              value={entry.keyword} onChange={e => changeOptions({ ...options, keywordRules: options.keywordRules.map((r, j) => j === i ? { ...r, keyword: e.target.value } : r) })} />
            <span aria-hidden="true">→</span>
            <input aria-label={t("fileManager.organize.table.target")} placeholder={t("fileManager.organize.table.target")}
              value={entry.folder} onChange={e => changeOptions({ ...options, keywordRules: options.keywordRules.map((r, j) => j === i ? { ...r, folder: e.target.value } : r) })} />
            <button type="button" className="icon-button" aria-label={t("fileManager.organize.removeRule")}
              onClick={() => changeOptions({ ...options, keywordRules: options.keywordRules.filter((_, j) => j !== i) })}><X size={14} /></button>
          </div>)}
        </fieldset>
      </div>}
      {step === "preview" && plan && <div className="org-step org-step--preview">
        <div className="org-preview-summary" aria-live="polite">
          <strong>{t("fileManager.organize.previewSummary.moves", { count: selected.length })}</strong>
          <span>{t("fileManager.organize.keepCount", { count: plan.moves.length - selected.length + plan.preservedFiles + plan.alreadyPlaced })}</span>
          {plan.conflictCount > 0 && <span className="org-preview-conflict">{t("fileManager.organize.previewSummary.conflicts", { count: plan.conflictCount })}</span>}
        </div>
        <p className="org-step-hint">{t("fileManager.organize.previewHint")}</p>
        <label className="org-conflict-control">{t("fileManager.organize.conflictStrategy")}
          <select value={options.conflictStrategy} disabled={busy} onChange={e => changeOptions({ ...options, conflictStrategy: e.target.value as "skip" | "rename" })}>
            <option value="skip">{t("fileManager.organize.skipConflicts")}</option>
            <option value="rename">{t("fileManager.organize.renameConflicts")}</option>
          </select>
        </label>
        <div className="org-preview-actions">
          <button type="button" className="button" disabled={busy} onClick={() => exclude(plan.moves.map(m => m.from), false)}>{t("fileManager.organize.selectAll")}</button>
          <button type="button" className="button" disabled={busy} onClick={() => exclude(plan.moves.map(m => m.from), true)}>{t("fileManager.organize.keepAll")}</button>
        </div>
        {dirty && <div className="org-dirty-notice" role="status"><span>{t("fileManager.organize.previewChanged")}</span>
          <button type="button" className="button button-primary" disabled={busy} aria-busy={planning} onClick={() => void preview()}>{planning && <ButtonSpinner />}{t(planning ? "fileManager.footer.generating" : "fileManager.organize.updatePreview")}</button></div>}
        {plan.moves.length === 0 && <div className="org-preview-empty"><CheckCircle size={24} /><p>{t("fileManager.organize.empty")}</p></div>}
        {groups.map(([key, moves]) => <PreviewGroup key={key} moves={moves} folderName={folderDrafts[key] ?? key}
          options={options} excluded={excluded} busy={busy} onOptions={changeOptions} onExcluded={exclude}
          onRename={name => {
            setFolderDrafts(previous => ({ ...previous, [key]: name }));
            const fileFolders = { ...options.fileFolders };
            for (const move of moves) fileFolders[move.from] = name;
            changeOptions({ ...options, fileFolders });
          }} />)}
      </div>}
      {planError && <p className="org-error" role="alert"><AlertTriangle size={14} />{planError}</p>}
      {step === "executing" && <>
        <StepExecuting progress={progress} result={result} error={execError} />
        {result && <p className="org-result-hint">{t("fileManager.organize.undoHint")}</p>}
      </>}
    </div>
    <div className="org-wizard-footer">
      {step !== "rule" && step !== "executing" && <button type="button" className="button" disabled={busy}
        onClick={() => { setPlanError(null); setStep(step === "preview" ? "dirs" : "rule"); }}><ChevronLeft size={14} />{t("fileManager.footer.back")}</button>}
      <span className="org-footer-hint">{step !== "executing" && t("fileManager.organize.preserveFooter")}</span>
      {step === "rule" && <button type="button" className="button button-primary" onClick={() => setStep("dirs")}>{t("fileManager.footer.next")}</button>}
      {step === "dirs" && <button type="button" className="button button-primary" disabled={busy || !sourceDir} aria-busy={planning} onClick={() => void preview()}>{planning && <ButtonSpinner />}{t(planning ? "fileManager.footer.generating" : "fileManager.footer.generate")}</button>}
      {step === "preview" && <button type="button" className="button button-primary" disabled={busy || dirty || selected.length === 0}
        onClick={() => void execute()}>{t("fileManager.organize.executeCount", { count: selected.length })}</button>}
      {step === "executing" && isDone && <button type="button" className="button button-primary" onClick={close}>{t("fileManager.footer.done")}</button>}
    </div>
  </dialog>;
}
