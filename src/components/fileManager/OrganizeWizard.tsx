// Smart Organizer Wizard — 4-step modal.
//
// Step 0: Choose rule (byCategory / byYear / byYearMonth / byExtension)
// Step 1: Choose source dir + destination dir
// Step 2: Preview diff table (from → to), with conflict warnings
// Step 3: Executing with live progress bar → done summary

import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  ArrowRight,
  CheckCircle,
  ChevronLeft,
  FolderOpen,
  Layers,
  Calendar,
  Tag,
  X,
  AlertTriangle,
  TriangleAlert,
} from "lucide-react";
import { chooseDirectory, planOrganize, executeOrganizePlan } from "../../lib/tauri";
import { errorMessage } from "../../lib/errors";
import { useTranslation } from "react-i18next";
import type {
  OrganizeMove,
  OrganizePlan,
  OrganizeProgress,
  OrganizeRule,
} from "../../types/fileManager";

// ── Rule definitions ──────────────────────────────────────────────────────────

const RULE_DEFS: { rule: OrganizeRule; key: "byCategory" | "byYear" | "byYearMonth" | "byExtension"; Icon: React.FC<{ size?: number }> }[] = [
  { rule: { kind: "byCategory" }, key: "byCategory", Icon: Layers },
  { rule: { kind: "byYear" }, key: "byYear", Icon: Calendar },
  { rule: { kind: "byYearMonth" }, key: "byYearMonth", Icon: Calendar },
  { rule: { kind: "byExtension" }, key: "byExtension", Icon: Tag },
];

// ── Step components ───────────────────────────────────────────────────────────

// Step 0: Pick a rule
function StepRule({
  selected,
  onSelect,
}: {
  selected: OrganizeRule | null;
  onSelect: (r: OrganizeRule) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="org-step">
      <p className="org-step-hint">{t("fileManager.organize.ruleHint")}</p>
      <div className="org-rule-grid">
        {RULE_DEFS.map((meta) => {
          const active = selected?.kind === meta.rule.kind;
          const rule = {
            label: t(`fileManager.organize.rules.${meta.key}.label`),
            description: t(`fileManager.organize.rules.${meta.key}.description`),
            example: t(`fileManager.organize.rules.${meta.key}.example`),
          };
          return (
            <button
              key={meta.rule.kind}
              type="button"
              className={`org-rule-card${active ? " org-rule-card--active" : ""}`}
              onClick={() => onSelect(meta.rule)}
            >
              <meta.Icon size={20} />
              <strong>{rule.label}</strong>
              <span>{rule.description}</span>
              <code>{rule.example}</code>
            </button>
          );
        })}
      </div>
    </div>
  );
}

// Step 1: Pick source + destination directories
function StepDirs({
  sourceDir,
  destDir,
  onSourceChange,
  onDestChange,
}: {
  sourceDir: string;
  destDir: string;
  onSourceChange: (p: string) => void;
  onDestChange: (p: string) => void;
}) {
  const { t } = useTranslation();
  async function pickSource() {
    const p = await chooseDirectory();
    if (p) onSourceChange(p);
  }
  async function pickDest() {
    const p = await chooseDirectory();
    if (p) onDestChange(p);
  }

  return (
    <div className="org-step">
      <p className="org-step-hint">{t("fileManager.organize.dirsHint")}</p>

      <label className="org-dir-label">
        <span>{t("fileManager.organize.sourceDir")}</span>
        <div className="org-dir-row">
          <span className="org-dir-path" title={sourceDir}>{sourceDir || t("fileManager.organize.notSelected")}</span>
          <button type="button" className="button button-primary org-dir-btn" onClick={pickSource}>
            <FolderOpen size={13} /> {t("fileManager.organize.choose")}
          </button>
        </div>
      </label>

      <label className="org-dir-label">
        <span>{t("fileManager.organize.destDir")}</span>
        <div className="org-dir-row">
          <span className="org-dir-path" title={destDir}>{destDir || t("fileManager.organize.notSelected")}</span>
          <button type="button" className="button org-dir-btn" onClick={pickDest}>
            <FolderOpen size={13} /> {t("fileManager.organize.choose")}
          </button>
        </div>
        <span className="org-dir-hint">{t("fileManager.organize.destSameHint")}</span>
      </label>
    </div>
  );
}

// Step 2: Preview plan
const PREVIEW_PAGE = 200;

function StepPreview({ plan }: { plan: OrganizePlan }) {
  const { t } = useTranslation();
  const [page, setPage] = useState(0);
  const totalPages = Math.ceil(plan.moves.length / PREVIEW_PAGE);
  const slice = plan.moves.slice(page * PREVIEW_PAGE, (page + 1) * PREVIEW_PAGE);

  return (
    <div className="org-step org-step--preview">
      <div className="org-preview-summary">
        <span><strong>{t("fileManager.organize.previewSummary.moves", { count: plan.moves.length })}</strong></span>
        {plan.alreadyPlaced > 0 && (
          <span className="org-preview-placed">{t("fileManager.organize.previewSummary.alreadyPlaced", { count: plan.alreadyPlaced })}</span>
        )}
        {plan.conflictCount > 0 && (
          <span className="org-preview-conflict">
            <AlertTriangle size={13} /> {t("fileManager.organize.previewSummary.conflicts", { count: plan.conflictCount })}
          </span>
        )}
      </div>

      {plan.moves.length === 0 ? (
        <div className="org-preview-empty">
          <CheckCircle size={24} />
          <p>{t("fileManager.organize.empty")}</p>
        </div>
      ) : (
        <>
          <div className="org-preview-table" role="table">
            <div className="org-preview-head" role="row">
              <span>{t("fileManager.organize.table.name")}</span>
              <span>{t("fileManager.organize.table.target")}</span>
              <span />
            </div>
            {slice.map((m) => (
              <MoveRow key={m.from} move={m} />
            ))}
          </div>
          {totalPages > 1 && (
            <div className="fm-pagination">
              <button type="button" className="button" disabled={page === 0} onClick={() => setPage(page - 1)}>{t("fileManager.pagination.previous")}</button>
              <span className="fm-pagination-info">{t("fileManager.pagination.info", { current: page + 1, total: totalPages })}</span>
              <button type="button" className="button" disabled={page >= totalPages - 1} onClick={() => setPage(page + 1)}>{t("fileManager.pagination.next")}</button>
            </div>
          )}
        </>
      )}
    </div>
  );
}

function MoveRow({ move }: { move: OrganizeMove }) {
  const { t } = useTranslation();
  return (
    <div
      className={`org-preview-row${move.conflict ? " org-preview-row--conflict" : ""}`}
      role="row"
      title={move.conflict ? t("fileManager.organize.conflictHint") : undefined}
    >
      <span className="org-preview-name" title={move.from}>{move.fromName}</span>
      <span className="org-preview-arrow">
        <ArrowRight size={12} />
        <code>{move.subfolder}/</code>
      </span>
      {move.conflict && (
        <span className="org-preview-conflict-badge" aria-label={t("fileManager.organize.conflictAria")}>
          <TriangleAlert size={12} />
        </span>
      )}
    </div>
  );
}

// Step 3: Executing + done
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

// ── Wizard root ────────────────────────────────────────────────────────────────

type Props = {
  open: boolean;
  scanId: string;
  defaultSourceDir: string;
  onClose: () => void;
  onDone: (operationId?: string) => void; // refresh list + persisted undo state
};

type WizardStep = "rule" | "dirs" | "preview" | "executing";

export function OrganizeWizard({ open, scanId, defaultSourceDir, onClose, onDone }: Props) {
  const { t } = useTranslation();
  const [step, setStep] = useState<WizardStep>("rule");
  const [selectedRule, setSelectedRule] = useState<OrganizeRule | null>(null);
  const [sourceDir, setSourceDir] = useState(defaultSourceDir);
  const [destDir, setDestDir] = useState(defaultSourceDir);
  const [planning, setPlanning] = useState(false);
  const [planError, setPlanError] = useState<string | null>(null);
  const [plan, setPlan] = useState<OrganizePlan | null>(null);
  const [progress, setProgress] = useState<OrganizeProgress | null>(null);
  const [result, setResult] = useState<{ succeeded: number; failed: { path: string; reason: string }[] } | null>(null);
  const [execError, setExecError] = useState<string | null>(null);
  const unlistenRef = useRef<(() => void) | null>(null);

  // Reset when opened
  useEffect(() => {
    if (open) {
      setStep("rule");
      setSelectedRule(null);
      setSourceDir(defaultSourceDir);
      setDestDir(defaultSourceDir);
      setPlan(null);
      setPlanError(null);
      setProgress(null);
      setResult(null);
      setExecError(null);
    }
  }, [open, defaultSourceDir]);

  // Clean up event listener on unmount
  useEffect(() => {
    return () => { unlistenRef.current?.(); };
  }, []);

  // Trap Escape key
  useEffect(() => {
    if (!open) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape" && step !== "executing") onClose();
    }
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, step, onClose]);

  if (!open) return null;

  // ── Navigation ──────────────────────────────────────────────────────────────

  async function goToPreview() {
    if (!selectedRule) return;
    const src = sourceDir;
    const dst = destDir || sourceDir;
    setPlanning(true);
    setPlanError(null);
    try {
      const p = await planOrganize(scanId, src, dst, selectedRule);
      setPlan(p);
      setStep("preview");
    } catch (err) {
      setPlanError(errorMessage(err));
    } finally {
      setPlanning(false);
    }
  }

  async function execute() {
    if (!plan || plan.moves.length === 0) {
      onDone();
      onClose();
      return;
    }
    setStep("executing");

    // Subscribe to progress events
    const unlisten = await listen<OrganizeProgress>("organize-progress", (ev) => {
      setProgress(ev.payload);
    });
    unlistenRef.current = unlisten;

    // Only send non-conflict moves
    const toMove = plan.moves
      .filter((m) => !m.conflict)
      .map((m) => ({
        from: m.from,
        to: m.to,
        expectedSizeBytes: m.expectedSizeBytes,
        expectedModifiedAt: m.expectedModifiedAt,
      }));

    try {
      const r = await executeOrganizePlan(scanId, toMove);
      setResult(r);
      onDone(r.operationId); // refresh file list + undo state
    } catch (err) {
      setExecError(errorMessage(err));
    } finally {
      unlisten();
      unlistenRef.current = null;
    }
  }

  // ── Step titles & actions ───────────────────────────────────────────────────

  const stepTitles: Record<WizardStep, string> = {
    rule: t("fileManager.steps.rule"),
    dirs: t("fileManager.steps.dirs"),
    preview: t("fileManager.steps.preview"),
    executing: t("fileManager.steps.executing"),
  };

  const isDone = result !== null || execError !== null;

  function renderFooter() {
    if (step === "executing") {
      return isDone ? (
        <button type="button" className="button button-primary" onClick={onClose}>
          {t("fileManager.footer.done")}
        </button>
      ) : null;
    }

    return (
      <>
        {step !== "rule" && (
          <button
            type="button"
            className="button"
            onClick={() => {
              if (step === "dirs") setStep("rule");
              else if (step === "preview") setStep("dirs");
            }}
          >
            <ChevronLeft size={14} /> {t("fileManager.footer.back")}
          </button>
        )}
        <div style={{ flex: 1 }} />
        {step === "rule" && (
          <button
            type="button"
            className="button button-primary"
            disabled={!selectedRule}
            onClick={() => setStep("dirs")}
          >
            {t("fileManager.footer.next")}
          </button>
        )}
        {step === "dirs" && (
          <button
            type="button"
            className="button button-primary"
            disabled={!sourceDir || planning}
            onClick={goToPreview}
          >
            {planning ? t("fileManager.footer.generating") : t("fileManager.footer.generate")}
          </button>
        )}
        {step === "preview" && (
          <button
            type="button"
            className="button button-primary"
            disabled={plan?.moves.length === 0}
            onClick={execute}
          >
            {t("fileManager.footer.execute")}
          </button>
        )}
      </>
    );
  }

  return (
    <div
      className="fm-dialog-backdrop"
      role="presentation"
      onClick={(e) => {
        if (e.target === e.currentTarget && step !== "executing") onClose();
      }}
    >
      <dialog className="org-wizard" open aria-modal aria-labelledby="org-wizard-title">
        {/* Header */}
        <div className="org-wizard-header">
          <h2 id="org-wizard-title" className="org-wizard-title">
            {stepTitles[step]}
          </h2>
          {step !== "executing" && (
            <button
              type="button"
              className="icon-button org-wizard-close"
              onClick={onClose}
              aria-label={t("fileManager.dialog.close")}
            >
              <X size={15} />
            </button>
          )}
        </div>

        {/* Step indicator */}
        <div className="org-steps-bar">
          {(["rule", "dirs", "preview", "executing"] as WizardStep[]).map((s, i) => (
            <span
              key={s}
              className={`org-step-dot${s === step ? " org-step-dot--active" : ""}${
                ["rule", "dirs", "preview", "executing"].indexOf(step) > i
                  ? " org-step-dot--done"
                  : ""
              }`}
            />
          ))}
        </div>

        {/* Body */}
        <div className="org-wizard-body">
          {step === "rule" && (
            <StepRule selected={selectedRule} onSelect={setSelectedRule} />
          )}
          {step === "dirs" && (
            <>
              <StepDirs
                sourceDir={sourceDir}
                destDir={destDir}
                onSourceChange={setSourceDir}
                onDestChange={setDestDir}
              />
              {planError && (
                <p className="org-error">
                  <AlertTriangle size={13} /> {planError}
                </p>
              )}
            </>
          )}
          {step === "preview" && plan && (
            <StepPreview plan={plan} />
          )}
          {step === "executing" && (
            <StepExecuting progress={progress} result={result} error={execError} />
          )}
        </div>

        {/* Footer */}
        <div className="org-wizard-footer">{renderFooter()}</div>
      </dialog>
    </div>
  );
}
