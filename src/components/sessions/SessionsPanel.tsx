// Agent sessions (Agent 会话): browse, filter and trash local AI-agent CLI
// sessions. Mirrors the disk-cleaner UX: insight-style collapsible groups,
// checkbox selection with a batch bar, ConfirmDialog, outcome banner.

import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import {
  Calendar,
  Check,
  ChevronRight,
  Copy,
  FolderOpen,
  Gem,
  History,
  RefreshCw,
  Search,
  Sparkles,
  Terminal,
  Trash2,
} from "lucide-react";
import { ConfirmDialog } from "../fileManager/ConfirmDialog";
import "../fileManager/FileManager.css"; // .fm-dialog styles for ConfirmDialog
import { errorMessage } from "../../lib/errors";
import { formatBytes, formatNumber, formatRelativeTime } from "../../lib/format";
import { discoverAgentSessions, trashAgentSessions } from "../../lib/tauri";
import type { AgentId, AgentSession, AgentSessionsReport } from "../../types/sessions";

// ── Agent meta ────────────────────────────────────────────────────────────────

type AgentMeta = { icon: typeof Sparkles; labelKey: string };

const AGENT_META: Record<AgentId, AgentMeta> = {
  claude: { icon: Sparkles, labelKey: "sessions.agentNames.claude" },
  codex: { icon: Terminal, labelKey: "sessions.agentNames.codex" },
  gemini: { icon: Gem, labelKey: "sessions.agentNames.gemini" },
};

function agentIcon(agent: string) {
  return (AGENT_META[agent as AgentId] ?? History).icon;
}

/** Dynamic i18n key with a known anchor, same pattern as the cleaner kinds. */
function agentLabel(agent: string, t: TFunction) {
  const meta = AGENT_META[agent as AgentId];
  return meta ? t(meta.labelKey as never) : agent;
}

// ── State ─────────────────────────────────────────────────────────────────────

type PanelState =
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "ready"; data: AgentSessionsReport };

type GroupBy = "project" | "time";

type SessionGroup = {
  key: string;
  name: string;
  path: string | null;
  sessions: AgentSession[];
};

type ExecResult = { succeeded: number; failed: { path: string; reason: string }[] };

const TIME_BUCKETS = ["today", "yesterday", "last7Days", "last30Days", "earlier"] as const;
type TimeBucket = (typeof TIME_BUCKETS)[number];

const DAY_SECONDS = 86_400;

// ── Component ─────────────────────────────────────────────────────────────────

export function SessionsPanel() {
  const { t } = useTranslation();

  const [state, setState] = useState<PanelState>({ status: "loading" });
  const [agentFilter, setAgentFilter] = useState<string>("all");
  const [query, setQuery] = useState("");
  const [groupBy, setGroupBy] = useState<GroupBy>("project");
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [confirmItems, setConfirmItems] = useState<AgentSession[] | null>(null);
  const [executing, setExecuting] = useState(false);
  const [result, setResult] = useState<ExecResult | null>(null);

  const refresh = useCallback(async () => {
    setState({ status: "loading" });
    try {
      const data = await discoverAgentSessions();
      setState({ status: "ready", data });
    } catch (error) {
      setState({ status: "error", message: errorMessage(error, t("sessions.loadError")) });
    }
  }, [t]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const sessions = state.status === "ready" ? state.data.sessions : [];

  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return sessions.filter((session) => {
      if (agentFilter !== "all" && session.agent !== agentFilter) return false;
      if (!needle) return true;
      return (
        session.title.toLowerCase().includes(needle) ||
        session.projectName.toLowerCase().includes(needle) ||
        session.projectDir.toLowerCase().includes(needle) ||
        session.firstMessage.toLowerCase().includes(needle) ||
        session.summary.toLowerCase().includes(needle) ||
        session.id.toLowerCase().includes(needle)
      );
    });
  }, [sessions, agentFilter, query]);

  const groups = useMemo(
    () => (groupBy === "project" ? groupByProject(filtered, t) : groupByTime(filtered, t)),
    [filtered, groupBy, t],
  );

  const agentCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const session of sessions) {
      counts.set(session.agent, (counts.get(session.agent) ?? 0) + 1);
    }
    return counts;
  }, [sessions]);

  const toggleGroup = useCallback((key: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }, []);

  const toggleSession = useCallback((path: string) => {
    setResult(null);
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, []);

  const setAllSelected = useCallback((selectAll: boolean) => {
    setResult(null);
    setSelected(() => (selectAll ? new Set(filtered.map((s) => s.sourcePath)) : new Set()));
  }, [filtered]);

  /** Group-level select-all: check selects every session in the group, uncheck clears them. */
  const toggleGroupSelection = useCallback((group: SessionGroup) => {
    setResult(null);
    setSelected((prev) => {
      const allSelected = group.sessions.every((session) => prev.has(session.sourcePath));
      const next = new Set(prev);
      for (const session of group.sessions) {
        if (allSelected) next.delete(session.sourcePath);
        else next.add(session.sourcePath);
      }
      return next;
    });
  }, []);

  const allSelected = filtered.length > 0 && filtered.every((s) => selected.has(s.sourcePath));
  const selectedSessions = useMemo(
    () => sessions.filter((session) => selected.has(session.sourcePath)),
    [sessions, selected],
  );

  const executeTrash = useCallback(async () => {
    const items = confirmItems;
    if (!items || executing) return;
    setExecuting(true);
    try {
      const outcome = await trashAgentSessions(
        items.map((session) => ({ path: session.sourcePath, sizeBytes: session.sizeBytes })),
      );
      setResult({ succeeded: outcome.succeeded.length, failed: outcome.failed });
      setSelected(new Set());
      await refresh();
    } catch (error) {
      setResult({
        succeeded: 0,
        failed: [{ path: "", reason: errorMessage(error, t("sessions.executeError")) }],
      });
    } finally {
      setExecuting(false);
      setConfirmItems(null);
    }
  }, [confirmItems, executing, refresh, t]);

  return (
    <section className="sessions-panel" aria-label={t("sessions.title")}>
      <header className="sessions-header">
        <div className="sessions-header-copy">
          <h1>
            <History size={18} aria-hidden="true" />
            {t("sessions.title")}
          </h1>
          <p className="text-secondary">{t("sessions.subtitle")}</p>
        </div>
      </header>

      <div className="sessions-toolbar">
        <select
          className="search-select sessions-agent-select"
          value={agentFilter}
          onChange={(event) => setAgentFilter(event.target.value)}
          aria-label={t("sessions.agentAll")}
        >
          <option value="all">
            {t("sessions.agentAll")} · {sessions.length}
          </option>
          {(state.status === "ready" ? state.data.agents : []).map((agent) => (
            <option key={agent.id} value={agent.id}>
              {agentLabel(agent.id, t)} · {agentCounts.get(agent.id) ?? agent.sessionCount}
            </option>
          ))}
        </select>

        <div className="sessions-search">
          <Search size={14} aria-hidden="true" />
          <input
            type="search"
            value={query}
            placeholder={t("sessions.searchPlaceholder")}
            onChange={(event) => setQuery(event.target.value)}
          />
        </div>

        <div className="sessions-group-toggle" role="group" aria-label={t("sessions.groupAria")}>
          <button type="button"
            className={`sessions-group-btn${groupBy === "time" ? " is-active" : ""}`}
            aria-pressed={groupBy === "time"}
            onClick={() => setGroupBy("time")}>
            {t("sessions.groupBy.time")}
          </button>
          <button type="button"
            className={`sessions-group-btn${groupBy === "project" ? " is-active" : ""}`}
            aria-pressed={groupBy === "project"}
            onClick={() => setGroupBy("project")}>
            {t("sessions.groupBy.project")}
          </button>
        </div>

        <button type="button" className="icon-button" title={t("sessions.refresh")}
          aria-label={t("sessions.refresh")} onClick={() => void refresh()}>
          <RefreshCw size={15} />
        </button>
      </div>

      {state.status === "error" ? (
        <div className="status-message status-error" role="alert">
          <History size={20} />
          <div><strong>{t("sessions.title")}</strong><p>{state.message}</p></div>
        </div>
      ) : state.status === "loading" ? (
        <div className="loading-state">{t("sessions.title")}…</div>
      ) : sessions.length === 0 ? (
        <div className="sessions-empty">
          <History size={32} aria-hidden="true" />
          <h2>{t("sessions.emptyTitle")}</h2>
          <p className="text-secondary">{t("sessions.emptyBody")}</p>
        </div>
      ) : (
        <>
          <div className="sessions-select-bar">
            <label className="cleaner-check">
              <input type="checkbox" checked={allSelected} onChange={(event) => setAllSelected(event.target.checked)} />
              {t("sessions.selectAll")}
            </label>
            <span className="text-secondary">
              {t("sessions.selectedCount", {
                count: formatNumber(selectedSessions.length),
                total: formatNumber(filtered.length),
              })}
            </span>
            <button type="button" className="button-danger cleaner-clean-button"
              disabled={executing || selectedSessions.length === 0}
              onClick={() => setConfirmItems(selectedSessions)}>
              <Trash2 size={14} aria-hidden="true" />
              {t("sessions.deleteSelected")}
            </button>
          </div>

          {result && <SessionsBanner result={result} />}

          <div className="file-list">
            {groups.length === 0 && <p className="empty-inline">{t("sessions.emptyFiltered")}</p>}
            {groups.map((group) => (
              <SessionGroupCard key={group.key} group={group} collapsed={collapsed.has(group.key)}
                selected={selected} showAgent={agentFilter === "all"}
                onToggleGroup={() => toggleGroup(group.key)}
                onToggleGroupSelection={toggleGroupSelection}
                onToggleSession={toggleSession}
                onDeleteOne={(session) => setConfirmItems([session])} />
            ))}
          </div>
        </>
      )}

      <ConfirmDialog
        open={confirmItems !== null}
        title={t("sessions.confirmTitle")}
        message={t("sessions.confirmMessage", { count: formatNumber(confirmItems?.length ?? 0) })}
        confirmLabel={t("sessions.confirmConfirm")}
        cancelLabel={t("sessions.cancelLabel")}
        variant="danger"
        onConfirm={() => void executeTrash()}
        onCancel={() => setConfirmItems(null)}
      />
    </section>
  );
}

// ── Grouping ──────────────────────────────────────────────────────────────────

function groupByProject(sessions: AgentSession[], t: TFunction): SessionGroup[] {
  const map = new Map<string, SessionGroup>();
  for (const session of sessions) {
    const key = session.projectDir || "__unknown__";
    let group = map.get(key);
    if (!group) {
      group = {
        key,
        name: session.projectName || t("sessions.unknownProject"),
        path: session.projectDir || null,
        sessions: [],
      };
      map.set(key, group);
    }
    group.sessions.push(session);
  }
  return sortGroups(map);
}

function groupByTime(sessions: AgentSession[], t: TFunction): SessionGroup[] {
  const map = new Map<string, SessionGroup>();
  for (const session of sessions) {
    const bucket = timeBucketOf(session);
    let group = map.get(bucket);
    if (!group) {
      group = {
        key: bucket,
        name: t(`sessions.timeBuckets.${bucket}` as "sessions.title"),
        path: null,
        sessions: [],
      };
      map.set(bucket, group);
    }
    group.sessions.push(session);
  }
  // Fixed chronological order instead of recency.
  return TIME_BUCKETS.filter((bucket) => map.has(bucket)).map((bucket) => map.get(bucket)!);
}

function timeBucketOf(session: AgentSession): TimeBucket {
  const ts = session.lastActiveAt ?? session.createdAt;
  if (!ts) return "earlier";
  const now = new Date();
  const startOfToday =
    new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime() / 1000;
  if (ts >= startOfToday) return "today";
  if (ts >= startOfToday - DAY_SECONDS) return "yesterday";
  if (ts >= startOfToday - 7 * DAY_SECONDS) return "last7Days";
  if (ts >= startOfToday - 30 * DAY_SECONDS) return "last30Days";
  return "earlier";
}

function sortGroups(map: Map<string, SessionGroup>): SessionGroup[] {
  const groups = [...map.values()];
  for (const group of groups) sortSessions(group.sessions);
  groups.sort((a, b) => lastActiveOf(b.sessions[0]) - lastActiveOf(a.sessions[0]));
  return groups;
}

function sortSessions(sessions: AgentSession[]): void {
  sessions.sort((a, b) => lastActiveOf(b) - lastActiveOf(a) || a.id.localeCompare(b.id));
}

function lastActiveOf(session: AgentSession): number {
  return session.lastActiveAt ?? session.createdAt ?? 0;
}

// ── Group card ────────────────────────────────────────────────────────────────

function SessionGroupCard({
  group, collapsed, selected, showAgent, onToggleGroup, onToggleGroupSelection,
  onToggleSession, onDeleteOne,
}: {
  group: SessionGroup;
  collapsed: boolean;
  selected: Set<string>;
  showAgent: boolean;
  onToggleGroup: () => void;
  onToggleGroupSelection: (group: SessionGroup) => void;
  onToggleSession: (path: string) => void;
  onDeleteOne: (session: AgentSession) => void;
}) {
  const { t } = useTranslation();
  const totalBytes = group.sessions.reduce((sum, session) => sum + session.sizeBytes, 0);
  const GroupIcon = group.path !== null ? FolderOpen : Calendar;
  const selectedCount = group.sessions.filter((session) => selected.has(session.sourcePath)).length;
  const allSelected = selectedCount === group.sessions.length;
  const someSelected = selectedCount > 0 && !allSelected;
  const checkRef = useCallback((element: HTMLInputElement | null) => {
    // The indeterminate state is DOM-only — it never round-trips through React.
    if (element) element.indeterminate = someSelected;
  }, [someSelected]);

  return (
    <div className="insight sessions-group">
      <div className="sessions-group-head">
        <input ref={checkRef} type="checkbox" className="cleaner-unit-check sessions-group-check"
          checked={allSelected} onChange={() => onToggleGroupSelection(group)}
          aria-label={t("sessions.selectAllGroup", { name: group.name })} />
        <button type="button" className="insight-head sessions-group-head-button"
          aria-expanded={!collapsed} onClick={onToggleGroup}>
          <span className="insight-icon"><GroupIcon size={15} /></span>
          <div className="insight-copy">
            <strong>{group.name}</strong>
            {group.path && <p className="text-secondary sessions-group-path">{group.path}</p>}
          </div>
          <div className="sessions-group-meta text-secondary">
            {t("sessions.sessionCount", { count: formatNumber(group.sessions.length) })}
            {" · "}
            {formatBytes(totalBytes)}
          </div>
          <ChevronRight className={`insight-chevron${collapsed ? "" : " is-open"}`} size={16} />
        </button>
      </div>

      {!collapsed && (
        <div className="insight-files">
          {group.sessions.map((session) => (
            <SessionRow key={session.sourcePath} session={session} checked={selected.has(session.sourcePath)}
              showAgent={showAgent} onToggle={() => onToggleSession(session.sourcePath)}
              onDelete={() => onDeleteOne(session)} />
          ))}
        </div>
      )}
    </div>
  );
}

// ── Session row ───────────────────────────────────────────────────────────────

function SessionRow({
  session, checked, showAgent, onToggle, onDelete,
}: {
  session: AgentSession;
  checked: boolean;
  showAgent: boolean;
  onToggle: () => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const AgentIcon = agentIcon(session.agent);
  const preview = session.summary || session.firstMessage || t("sessions.untitledSession");

  const copyResume = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(session.resumeCommand);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch {
      // Clipboard unavailable (rare in the Tauri webview) — ignore.
    }
  }, [session.resumeCommand]);

  return (
    <div className="file-row session-row">
      <input type="checkbox" className="cleaner-unit-check" checked={checked}
        onChange={onToggle}
        aria-label={session.title || session.id} />
      <div className="file-copy">
        <strong title={session.title || session.id}>
          {showAgent && (
            <span className="session-agent-badge" title={agentLabel(session.agent, t)}>
              <AgentIcon size={11} />
              {agentLabel(session.agent, t)}
            </span>
          )}
          {session.title || t("sessions.untitled")}
        </strong>
        <span title={preview}>{preview}</span>
      </div>
      <div className="file-meta">
        <strong title={t("sessions.lastActiveAt", { time: formatRelativeTime(session.lastActiveAt ?? session.createdAt) })}>
          {formatRelativeTime(session.lastActiveAt ?? session.createdAt)}
        </strong>
        <span>{formatBytes(session.sizeBytes)}</span>
      </div>
      <button type="button" className="icon-button" title={t("sessions.copyResume")}
        aria-label={t("sessions.copyResume")} onClick={() => void copyResume()}>
        {copied ? <Check size={15} /> : <Copy size={15} />}
      </button>
      <button type="button" className="icon-button session-delete" title={t("sessions.deleteOne")}
        aria-label={t("sessions.deleteOne")} onClick={onDelete}>
        <Trash2 size={15} />
      </button>
    </div>
  );
}

// ── Execution result banner ───────────────────────────────────────────────────

function SessionsBanner({ result }: { result: ExecResult }) {
  const { t } = useTranslation();
  return (
    <div className="cleaner-result sessions-result" role="status">
      {result.succeeded > 0 && (
        <p>{t("sessions.outcomeSummary", { succeeded: formatNumber(result.succeeded) })}</p>
      )}
      {result.failed.length > 0 && (
        <div>
          <p>{t("sessions.outcomeFailed", { count: formatNumber(result.failed.length) })}</p>
          <ul>
            {result.failed.map((failure) => (
              <li key={failure.path || failure.reason}>
                <span>{failure.path || "—"}</span>
                <span>{failure.reason}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
