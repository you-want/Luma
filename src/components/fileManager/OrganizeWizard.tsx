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
import type {
  OrganizeMove,
  OrganizePlan,
  OrganizeProgress,
  OrganizeRule,
} from "../../types/fileManager";

// ── Rule definitions ──────────────────────────────────────────────────────────

type RuleMeta = {
  rule: OrganizeRule;
  label: string;
  description: string;
  example: string;
  Icon: React.FC<{ size?: number }>;
};

const RULES: RuleMeta[] = [
  {
    rule: { kind: "byCategory" },
    label: "按文件类型",
    description: "将文件按 Luma 识别的类型分组",
    example: "图片/ · 视频/ · 文档/ · 代码/",
    Icon: Layers,
  },
  {
    rule: { kind: "byYear" },
    label: "按年份",
    description: "将文件按修改时间的年份归档",
    example: "2023/ · 2024/ · 2025/",
    Icon: Calendar,
  },
  {
    rule: { kind: "byYearMonth" },
    label: "按年月",
    description: "将文件按修改时间的年份和月份归档",
    example: "2024-01/ · 2024-12/ · 2025-03/",
    Icon: Calendar,
  },
  {
    rule: { kind: "byExtension" },
    label: "按扩展名",
    description: "将文件按小写扩展名分组",
    example: "pdf/ · jpg/ · mp4/ · no-extension/",
    Icon: Tag,
  },
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
  return (
    <div className="org-step">
      <p className="org-step-hint">选择整理规则，Luma 将按该规则把文件分配到子目录。</p>
      <div className="org-rule-grid">
        {RULES.map((meta) => {
          const active = selected?.kind === meta.rule.kind;
          return (
            <button
              key={meta.rule.kind}
              type="button"
              className={`org-rule-card${active ? " org-rule-card--active" : ""}`}
              onClick={() => onSelect(meta.rule)}
            >
              <meta.Icon size={20} />
              <strong>{meta.label}</strong>
              <span>{meta.description}</span>
              <code>{meta.example}</code>
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
      <p className="org-step-hint">选择要整理的目录（源），以及整理后存放的目标目录。目标可与源相同（在原地创建子目录）。</p>

      <label className="org-dir-label">
        <span>源目录</span>
        <div className="org-dir-row">
          <span className="org-dir-path" title={sourceDir}>{sourceDir || "未选择"}</span>
          <button type="button" className="button button-primary org-dir-btn" onClick={pickSource}>
            <FolderOpen size={13} /> 选择…
          </button>
        </div>
      </label>

      <label className="org-dir-label">
        <span>目标目录</span>
        <div className="org-dir-row">
          <span className="org-dir-path" title={destDir}>{destDir || "未选择"}</span>
          <button type="button" className="button org-dir-btn" onClick={pickDest}>
            <FolderOpen size={13} /> 选择…
          </button>
        </div>
        <span className="org-dir-hint">留空时与源目录相同（原地整理）</span>
      </label>
    </div>
  );
}

// Step 2: Preview plan
const PREVIEW_PAGE = 200;

function StepPreview({ plan }: { plan: OrganizePlan }) {
  const [page, setPage] = useState(0);
  const totalPages = Math.ceil(plan.moves.length / PREVIEW_PAGE);
  const slice = plan.moves.slice(page * PREVIEW_PAGE, (page + 1) * PREVIEW_PAGE);

  return (
    <div className="org-step org-step--preview">
      <div className="org-preview-summary">
        <span><strong>{plan.moves.length}</strong> 个文件将被移动</span>
        {plan.alreadyPlaced > 0 && (
          <span className="org-preview-placed">已在正确位置：{plan.alreadyPlaced} 个（跳过）</span>
        )}
        {plan.conflictCount > 0 && (
          <span className="org-preview-conflict">
            <AlertTriangle size={13} /> {plan.conflictCount} 个冲突（目标已存在，将跳过）
          </span>
        )}
      </div>

      {plan.moves.length === 0 ? (
        <div className="org-preview-empty">
          <CheckCircle size={24} />
          <p>所有文件已在正确位置，无需移动。</p>
        </div>
      ) : (
        <>
          <div className="org-preview-table" role="table">
            <div className="org-preview-head" role="row">
              <span>文件名</span>
              <span>目标子目录</span>
              <span />
            </div>
            {slice.map((m) => (
              <MoveRow key={m.from} move={m} />
            ))}
          </div>
          {totalPages > 1 && (
            <div className="fm-pagination">
              <button type="button" className="button" disabled={page === 0} onClick={() => setPage(page - 1)}>上一页</button>
              <span className="fm-pagination-info">第 {page + 1} / {totalPages} 页</span>
              <button type="button" className="button" disabled={page >= totalPages - 1} onClick={() => setPage(page + 1)}>下一页</button>
            </div>
          )}
        </>
      )}
    </div>
  );
}

function MoveRow({ move }: { move: OrganizeMove }) {
  return (
    <div
      className={`org-preview-row${move.conflict ? " org-preview-row--conflict" : ""}`}
      role="row"
      title={move.conflict ? "目标路径已存在，将跳过此文件" : undefined}
    >
      <span className="org-preview-name" title={move.from}>{move.fromName}</span>
      <span className="org-preview-arrow">
        <ArrowRight size={12} />
        <code>{move.subfolder}/</code>
      </span>
      {move.conflict && (
        <span className="org-preview-conflict-badge" aria-label="冲突">
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
        <strong>整理完成</strong>
        <p>成功移动 {result.succeeded} 个文件</p>
        {result.failed.length > 0 && (
          <div className="org-exec-failures">
            <p className="org-exec-fail-title">{result.failed.length} 个文件未能移动：</p>
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
    rule: "选择整理规则",
    dirs: "选择目录",
    preview: "预览变更",
    executing: "正在整理…",
  };

  const isDone = result !== null || execError !== null;

  function renderFooter() {
    if (step === "executing") {
      return isDone ? (
        <button type="button" className="button button-primary" onClick={onClose}>
          完成
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
            <ChevronLeft size={14} /> 上一步
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
            下一步
          </button>
        )}
        {step === "dirs" && (
          <button
            type="button"
            className="button button-primary"
            disabled={!sourceDir || planning}
            onClick={goToPreview}
          >
            {planning ? "生成预览中…" : "生成预览"}
          </button>
        )}
        {step === "preview" && (
          <button
            type="button"
            className="button button-primary"
            disabled={plan?.moves.length === 0}
            onClick={execute}
          >
            执行整理
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
              aria-label="关闭"
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
