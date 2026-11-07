// 磁盘管家 (Disk Cleaner): reclaim space at registered fixed locations.
//
// Product contract (see docs/disk-cleaner.md):
//   - Works without scanning a directory first — locations are fixed.
//   - Every deletion routes to the OS trash; nothing is ever permanent.
//   - The whitelist hides protected paths from suggestions and execution.
//   - The trash itself is only counted, never emptied from here.

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ChevronRight,
  Code2,
  Database,
  Download,
  FileText,
  FolderOpen,
  Globe,
  HardDrive,
  History,
  Layers,
  Package,
  Plus,
  RefreshCw,
  ShieldCheck,
  Smartphone,
  Trash2,
  X,
} from "lucide-react";
import { ConfirmDialog } from "../fileManager/ConfirmDialog";
import { MaintenancePanel } from "./MaintenancePanel";
import { OrphanPanel } from "./OrphanPanel";
import { PurgePanel } from "./PurgePanel";
import { SlimPanel } from "./SlimPanel";
import { SystemStatusCard } from "./SystemStatusCard";
import {
  addCleanerWhitelist,
  chooseDirectory,
  executeCleaner,
  getCleanerOverview,
  getCleanerWhitelist,
  listCleanerFiles,
  listCleanerOperations,
  removeCleanerWhitelist,
  revealPath,
} from "../../lib/tauri";
import { formatBytes, formatDate, formatNumber } from "../../lib/format";
import { errorMessage } from "../../lib/errors";
import type {
  CleanerFile,
  CleanerLocation,
  CleanerOperation,
  CleanerOverview,
  CleanerRisk,
  CleanerWhitelistEntry,
  CleanFailure,
  VolumeInfo,
} from "../../types/scan";

// ── Icon map ──────────────────────────────────────────────────────────────────

const KIND_ICON: Record<string, typeof Trash2> = {
  userCaches: Database,
  appLogs: FileText,
  browserCaches: Globe,
  pkgManagerCaches: Layers,
  devDerived: Code2,
  simDevices: Smartphone,
  installers: Package,
  oldDownloads: Download,
  trash: Trash2,
};

const RISK_COLOR: Record<CleanerRisk, string> = {
  safe: "var(--han-color-accent)",
  caution: "var(--han-color-warning)",
  review: "var(--han-color-error)",
};

// ── Local state ───────────────────────────────────────────────────────────────

type OverviewState =
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "ready"; data: CleanerOverview };

type UnitsState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "ready"; units: CleanerFile[] };

type ExecResult = { kind: string; succeeded: number; failed: CleanFailure[] };

// ── Component ─────────────────────────────────────────────────────────────────

export function CleanerCenter() {
  const { t } = useTranslation();

  const [overview, setOverview] = useState<OverviewState>({ status: "loading" });
  const [expandedKind, setExpandedKind] = useState<string | null>(null);
  const [units, setUnits] = useState<Record<string, UnitsState>>({});
  const [selected, setSelected] = useState<Record<string, Set<string>>>({});
  const [confirmKind, setConfirmKind] = useState<string | null>(null);
  const [executing, setExecuting] = useState(false);
  const [result, setResult] = useState<ExecResult | null>(null);
  const [whitelistOpen, setWhitelistOpen] = useState(false);
  const [whitelist, setWhitelist] = useState<CleanerWhitelistEntry[] | null>(null);
  const [whitelistBusy, setWhitelistBusy] = useState(false);
  const [whitelistError, setWhitelistError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setOverview({ status: "loading" });
    try {
      const data = await getCleanerOverview();
      setOverview({ status: "ready", data });
    } catch (error) {
      setOverview({ status: "error", message: errorMessage(error, t("cleaner.loadError")) });
    }
  }, [t]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const toggleWhitelist = useCallback(async () => {
    const next = !whitelistOpen;
    setWhitelistOpen(next);
    if (next && whitelist === null) {
      try {
        setWhitelist(await getCleanerWhitelist());
      } catch (error) {
        setWhitelist([]);
        setOverview({ status: "error", message: errorMessage(error, t("cleaner.loadError")) });
      }
    }
  }, [whitelistOpen, whitelist, t]);

  const addProtectedPath = useCallback(async () => {
    if (whitelistBusy) return;
    setWhitelistBusy(true);
    setWhitelistError(null);
    try {
      const path = await chooseDirectory();
      if (!path) return;
      setWhitelist(await addCleanerWhitelist(path));
    } catch (error) {
      setWhitelistError(errorMessage(error, t("cleaner.whitelist.addFailed")));
    } finally {
      setWhitelistBusy(false);
    }
  }, [whitelistBusy, t]);

  const removeProtectedPath = useCallback(async (path: string) => {
    if (whitelistBusy) return;
    setWhitelistBusy(true);
    setWhitelistError(null);
    try {
      setWhitelist(await removeCleanerWhitelist(path));
    } catch (error) {
      setWhitelistError(errorMessage(error, t("cleaner.whitelist.addFailed")));
    } finally {
      setWhitelistBusy(false);
    }
  }, [whitelistBusy, t]);

  const toggleKind = useCallback((kind: string) => {
    setResult(null);
    if (expandedKind === kind) {
      setExpandedKind(null);
      return;
    }
    setExpandedKind(kind);
    const current = units[kind];
    if (current && current.status !== "idle") return;

    setUnits((prev) => ({ ...prev, [kind]: { status: "loading" } }));
    listCleanerFiles(kind, 50)
      .then((rows) => {
        setUnits((prev) => ({ ...prev, [kind]: { status: "ready", units: rows } }));
        // Default to selecting everything the user is about to review.
        setSelected((prev) => ({
          ...prev,
          [kind]: new Set(rows.map((row) => row.path)),
        }));
      })
      .catch((error) => {
        setUnits((prev) => ({
          ...prev,
          [kind]: { status: "error", message: errorMessage(error, t("cleaner.unitsError")) },
        }));
      });
  }, [expandedKind, units, t]);

  const toggleUnit = useCallback((kind: string, path: string) => {
    setSelected((prev) => {
      const next = new Set(prev[kind] ?? []);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return { ...prev, [kind]: next };
    });
  }, []);

  const setAllSelected = useCallback((kind: string, selectAll: boolean, all: CleanerFile[]) => {
    setSelected((prev) => ({
      ...prev,
      [kind]: selectAll ? new Set(all.map((row) => row.path)) : new Set(),
    }));
  }, []);

  const confirmClean = useCallback(async () => {
    const kind = confirmKind;
    if (!kind || executing) return;
    const state = units[kind];
    if (state.status !== "ready") return;
    const chosen = state.units.filter((unit) => selected[kind]?.has(unit.path));
    if (chosen.length === 0) {
      setConfirmKind(null);
      return;
    }

    setExecuting(true);
    try {
      const outcome = await executeCleaner(
        kind,
        chosen.map((unit) => ({ path: unit.path, sizeBytes: unit.sizeBytes })),
      );
      setResult({ kind, succeeded: outcome.succeeded.length, failed: outcome.failed });
      // Refresh stats and re-list: cleaned units are gone from the filesystem.
      await refresh();
      if (expandedKind === kind) {
        setUnits((prev) => ({ ...prev, [kind]: { status: "loading" } }));
        const rows = await listCleanerFiles(kind, 50);
        setUnits((prev) => ({ ...prev, [kind]: { status: "ready", units: rows } }));
        setSelected((prev) => ({ ...prev, [kind]: new Set(rows.map((row) => row.path)) }));
      }
    } catch (error) {
      setResult({
        kind,
        succeeded: 0,
        failed: [{ path: "", reason: errorMessage(error, t("cleaner.executeError")) }],
      });
    } finally {
      setExecuting(false);
      setConfirmKind(null);
    }
  }, [confirmKind, executing, units, selected, refresh, expandedKind, t]);

  // ── Render ──────────────────────────────────────────────────────────────────

  return (
    <section className="result-section cleaner-section" aria-labelledby="cleaner-title">
      <div className="section-heading compact-heading">
        <h2 id="cleaner-title">{t("cleaner.title")}</h2>
        <button type="button" className="icon-button" title={t("cleaner.refresh")}
          aria-label={t("cleaner.refresh")} disabled={overview.status === "loading"}
          onClick={() => void refresh()}>
          <RefreshCw size={15} />
        </button>
      </div>
      <p className="cleaner-subtitle text-secondary">{t("cleaner.subtitle")}</p>

      {overview.status === "loading" && <p className="empty-inline">{t("cleaner.loading")}</p>}
      {overview.status === "error" && (
        <p className="empty-inline cleaner-error" role="alert">{overview.message}</p>
      )}

      {overview.status === "ready" && (
        <>
          {overview.data.locations.length > 0 && (
            <p className="cleaner-health">
              {t("cleaner.healthSummary", {
                size: formatBytes(
                  overview.data.locations.reduce((sum, location) => sum + location.sizeBytes, 0),
                ),
                count: formatNumber(overview.data.locations.length),
              })}
            </p>
          )}

          <SystemStatusCard />

          {overview.data.volumes.length > 0 && (
            <div className="cleaner-volumes">
              {overview.data.volumes.map((volume) => (
                <VolumeBar key={volume.mountPoint} volume={volume} />
              ))}
            </div>
          )}

          {overview.data.locations.length === 0 ? (
            <p className="empty-inline">{t("cleaner.empty")}</p>
          ) : (
            <div className="cleanup-list">
              {overview.data.locations.map((location) => (
                <CleanerLocationRow
                  key={location.kind}
                  location={location}
                  expanded={expandedKind === location.kind}
                  unitsState={units[location.kind] ?? { status: "idle" }}
                  selectedPaths={selected[location.kind] ?? new Set()}
                  executing={executing}
                  result={result?.kind === location.kind ? result : null}
                  onToggle={() => toggleKind(location.kind)}
                  onToggleUnit={(path) => toggleUnit(location.kind, path)}
                  onSetAll={(selectAll) => {
                    const state = units[location.kind];
                    if (state.status === "ready") {
                      setAllSelected(location.kind, selectAll, state.units);
                    }
                  }}
                  onClean={() => setConfirmKind(location.kind)}
                />
              ))}
            </div>
          )}

          <PurgePanel />

          <OrphanPanel />

          <SlimPanel />

          <MaintenancePanel />

          <WhitelistPanel
            open={whitelistOpen}
            entries={whitelist}
            busy={whitelistBusy}
            error={whitelistError}
            onToggle={() => void toggleWhitelist()}
            onAdd={() => void addProtectedPath()}
            onRemove={(path) => void removeProtectedPath(path)}
          />

          <CleanerHistoryPanel />

          <p className="insight-note">{t("cleaner.note")}</p>
        </>
      )}

      <ConfirmDialog
        open={confirmKind !== null}
        title={t("cleaner.cleanConfirmTitle")}
        message={t("cleaner.cleanConfirmBody", {
          count: confirmKind ? selectedCountFor(confirmKind, units, selected) : 0,
          size: formatBytes(selectedBytesFor(confirmKind, units, selected)),
        })}
        confirmLabel={t("cleaner.confirmLabel")}
        cancelLabel={t("cleaner.cancelLabel")}
        variant="danger"
        onConfirm={() => void confirmClean()}
        onCancel={() => setConfirmKind(null)}
      />
    </section>
  );
}

// Helpers over the selected-units state, shared by the confirm dialog copy.

function selectedUnits(
  kind: string | null,
  units: Record<string, UnitsState>,
  selected: Record<string, Set<string>>,
): CleanerFile[] {
  if (!kind) return [];
  const state = units[kind];
  if (state.status !== "ready") return [];
  return state.units.filter((unit) => selected[kind]?.has(unit.path));
}

function selectedCountFor(
  kind: string | null,
  units: Record<string, UnitsState>,
  selected: Record<string, Set<string>>,
): number {
  return selectedUnits(kind, units, selected).length;
}

function selectedBytesFor(
  kind: string | null,
  units: Record<string, UnitsState>,
  selected: Record<string, Set<string>>,
): number {
  return selectedUnits(kind, units, selected).reduce((sum, unit) => sum + unit.sizeBytes, 0);
}

// ── Volume bar ────────────────────────────────────────────────────────────────

function VolumeBar({ volume }: { volume: VolumeInfo }) {
  const { t } = useTranslation();
  const percent = volume.totalBytes > 0
    ? Math.min(100, Math.round((volume.usedBytes / volume.totalBytes) * 100))
    : 0;
  const fillClass = percent >= 90
    ? "cleaner-volume-fill is-danger"
    : percent >= 75
      ? "cleaner-volume-fill is-warning"
      : "cleaner-volume-fill";
  const name = volume.mountPoint === "/" || volume.name === ""
    ? t("cleaner.systemVolume")
    : volume.name;

  return (
    <div className="cleaner-volume">
      <div className="cleaner-volume-head">
        <HardDrive size={15} aria-hidden="true" />
        <strong>{name}</strong>
        <span className="text-secondary">
          {t("cleaner.volumeUsed", {
            used: formatBytes(volume.usedBytes),
            total: formatBytes(volume.totalBytes),
            free: formatBytes(volume.freeBytes),
          })}
        </span>
      </div>
      <div
        className="cleaner-volume-track"
        role="meter"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent}
        aria-label={t("cleaner.volumeAria", { name })}
      >
        <div className={fillClass} style={{ width: `${percent}%` }} />
      </div>
    </div>
  );
}

// ── Location row ──────────────────────────────────────────────────────────────

type LocationProps = {
  location: CleanerLocation;
  expanded: boolean;
  unitsState: UnitsState;
  selectedPaths: Set<string>;
  executing: boolean;
  result: ExecResult | null;
  onToggle: () => void;
  onToggleUnit: (path: string) => void;
  onSetAll: (selectAll: boolean) => void;
  onClean: () => void;
};

function CleanerLocationRow({
  location, expanded, unitsState, selectedPaths, executing, result,
  onToggle, onToggleUnit, onSetAll, onClean,
}: LocationProps) {
  const { t } = useTranslation();
  const Icon = KIND_ICON[location.kind] ?? FolderOpen;
  const executable = location.kind !== "trash";
  const kindLabel = t(`cleaner.kinds.${location.kind}` as "cleaner.title");
  const basis = String(
    t(`cleaner.basis.${location.kind}` as "cleaner.title", { days: 180 } as never),
  );
  const riskLabel = t(`cleaner.risk.${location.risk}` as "cleaner.title");
  const riskColor = RISK_COLOR[location.risk];
  const allSelected = unitsState.status === "ready"
    && unitsState.units.length > 0
    && unitsState.units.every((unit) => selectedPaths.has(unit.path));

  return (
    <div className="insight cleaner-row">
      <button type="button" className="insight-head" aria-expanded={expanded} onClick={onToggle}>
        <span className="insight-icon"
          style={{ color: riskColor, background: `color-mix(in srgb, ${riskColor} 14%, transparent)` }}>
          <Icon size={15} />
        </span>
        <div className="insight-copy">
          <strong>
            {kindLabel}
            <span className="cleaner-risk-badge" style={{
              color: riskColor,
              background: `color-mix(in srgb, ${riskColor} 12%, transparent)`,
            }}>{riskLabel}</span>
          </strong>
          <p className="text-secondary">
            {t("cleaner.summary", {
              count: formatNumber(location.fileCount),
              size: formatBytes(location.sizeBytes),
            })}
            {" — "}
            {basis}
            {location.isPartial && <span className="cleaner-partial"> · {t("cleaner.partial")}</span>}
          </p>
        </div>
        <ChevronRight className={`insight-chevron${expanded ? " is-open" : ""}`} size={16} />
      </button>

      {expanded && (
        <div className="insight-files">
          {result && <ExecBanner result={result} />}
          {unitsState.status === "idle" || unitsState.status === "loading" ? (
            <p className="empty-inline">{t("cleaner.unitsLoading")}</p>
          ) : unitsState.status === "error" ? (
            <p className="empty-inline">{unitsState.message}</p>
          ) : unitsState.units.length === 0 ? (
            <p className="empty-inline">{t("cleaner.unitsEmpty")}</p>
          ) : (
            <>
              {executable && (
                <div className="cleaner-select-bar">
                  <label className="cleaner-check">
                    <input type="checkbox" checked={allSelected}
                      onChange={(event) => onSetAll(event.target.checked)} />
                    {t("cleaner.selectAll")}
                  </label>
                  <button type="button" className="button-primary cleaner-clean-button"
                    disabled={executing || selectedPaths.size === 0} onClick={onClean}>
                    {executing
                      ? t("cleaner.executing")
                      : t("cleaner.cleanSelected", {
                        count: formatNumber(selectedPaths.size),
                        size: formatBytes(
                          unitsState.units
                            .filter((unit) => selectedPaths.has(unit.path))
                            .reduce((sum, unit) => sum + unit.sizeBytes, 0),
                        ),
                      })}
                  </button>
                </div>
              )}
              <div className="file-list">
                {unitsState.units.map((unit) => (
                  <CleanerUnitRow key={unit.path} unit={unit} kind={location.kind}
                    checked={selectedPaths.has(unit.path)} executable={executable}
                    onToggle={() => onToggleUnit(unit.path)} />
                ))}
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}

// ── Unit row ──────────────────────────────────────────────────────────────────

function CleanerUnitRow({
  unit, kind, checked, executable, onToggle,
}: {
  unit: CleanerFile;
  kind: string;
  checked: boolean;
  executable: boolean;
  onToggle: () => void;
}) {
  const { t } = useTranslation();

  return (
    <div className="file-row cleaner-unit-row">
      {executable ? (
        <input type="checkbox" className="cleaner-unit-check" checked={checked}
          onChange={onToggle}
          aria-label={t("cleaner.unitAria", { name: unit.name })} />
      ) : (
        <span className="cleaner-unit-readonly" title={t("cleaner.trashViewOnly")} />
      )}
      <div className="file-copy">
        <strong title={unit.name}>{unit.name}</strong>
        <span title={unit.path}>{unit.path}</span>
      </div>
      <div className="file-meta">
        <strong>{formatBytes(unit.sizeBytes)}</strong>
        <span>{unit.modifiedAt ? formatDate(unit.modifiedAt) : t("common.missing")}</span>
      </div>
      <button type="button" className="icon-button" title={t("common.reveal")}
        aria-label={t("common.revealNamed", { name: unit.name })}
        onClick={() => void revealPath(unit.path).catch(() => undefined)}>
        <ChevronRight size={16} />
      </button>
      {kind === "trash" && <span className="visually-hidden">{t("cleaner.trashViewOnly")}</span>}
    </div>
  );
}

// ── Execution result banner ───────────────────────────────────────────────────

function ExecBanner({ result }: { result: ExecResult }) {
  const { t } = useTranslation();
  return (
    <div className="cleaner-result" role="status">
      {result.succeeded > 0 && (
        <p>{t("cleaner.resultDone", { count: formatNumber(result.succeeded) })}</p>
      )}
      {result.failed.length > 0 && (
        <div>
          <p>{t("cleaner.resultFailed", { count: formatNumber(result.failed.length) })}</p>
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

// ── Whitelist panel ───────────────────────────────────────────────────────────

function WhitelistPanel({
  open, entries, busy, error, onToggle, onAdd, onRemove,
}: {
  open: boolean;
  entries: CleanerWhitelistEntry[] | null;
  busy: boolean;
  error: string | null;
  onToggle: () => void;
  onAdd: () => void;
  onRemove: (path: string) => void;
}) {
  const { t } = useTranslation();
  const count = entries?.length ?? 0;

  return (
    <div className="cleaner-whitelist">
      <button type="button" className="cleaner-whitelist-head" aria-expanded={open} onClick={onToggle}>
        <ShieldCheck size={15} aria-hidden="true" />
        <strong>{t("cleaner.whitelist.title")}{count > 0 ? ` (${formatNumber(count)})` : ""}</strong>
        <ChevronRight className={`insight-chevron${open ? " is-open" : ""}`} size={15} />
      </button>
      {open && (
        <div className="cleaner-whitelist-body">
          <p className="text-secondary">{t("cleaner.whitelist.note")}</p>
          {error && <p className="empty-inline cleaner-error" role="alert">{error}</p>}
          {entries === null ? (
            <p className="empty-inline">{t("cleaner.loading")}</p>
          ) : entries.length === 0 ? (
            <p className="empty-inline">{t("cleaner.whitelist.empty")}</p>
          ) : (
            <ul className="cleaner-whitelist-list">
              {entries.map((entry) => (
                <li key={entry.path} className="cleaner-whitelist-row">
                  <span title={entry.path}>{entry.path}</span>
                  <button type="button" className="icon-button" disabled={busy}
                    title={t("cleaner.whitelist.remove")}
                    aria-label={t("cleaner.whitelist.removeAria", { path: entry.path })}
                    onClick={() => onRemove(entry.path)}>
                    <X size={14} />
                  </button>
                </li>
              ))}
            </ul>
          )}
          <button type="button" className="cleaner-add-button" disabled={busy} onClick={onAdd}>
            <Plus size={14} aria-hidden="true" />
            {busy ? t("cleaner.whitelist.adding") : t("cleaner.whitelist.add")}
          </button>
        </div>
      )}
    </div>
  );
}

// ── Cleaner history panel (CLN-303) ───────────────────────────────────────────

const HISTORY_STATUS: Record<string, string> = {
  completed: "cleaner.history.status.completed",
  partial: "cleaner.history.status.partial",
  failed: "cleaner.history.status.failed",
  running: "cleaner.history.status.running",
  interrupted: "cleaner.history.status.interrupted",
};

const HISTORY_STATUS_COLOR: Record<string, string> = {
  completed: "var(--han-color-accent)",
  partial: "var(--han-color-warning)",
  failed: "var(--han-color-error)",
  running: "var(--han-color-info)",
  interrupted: "var(--text-secondary)",
};

function CleanerHistoryPanel() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [entries, setEntries] = useState<CleanerOperation[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const toggle = useCallback(() => {
    const next = !open;
    setOpen(next);
    if (next) {
      setError(null);
      listCleanerOperations(20)
        .then(setEntries)
        .catch((err) => setError(errorMessage(err, t("cleaner.history.loadError"))));
    }
  }, [open, t]);

  return (
    <div className="cleaner-whitelist cleaner-history">
      <button type="button" className="cleaner-whitelist-head" aria-expanded={open} onClick={toggle}>
        <History size={15} aria-hidden="true" />
        <strong>{t("cleaner.history.title")}</strong>
        <ChevronRight className={`insight-chevron${open ? " is-open" : ""}`} size={15} />
      </button>
      {open && (
        <div className="cleaner-whitelist-body">
          {error && <p className="empty-inline cleaner-error" role="alert">{error}</p>}
          {entries === null && !error && (
            <p className="empty-inline">{t("cleaner.history.loading")}</p>
          )}
          {entries !== null && entries.length === 0 && !error && (
            <p className="empty-inline">{t("cleaner.history.empty")}</p>
          )}
          {entries !== null && entries.length > 0 && (
            <ul className="cleaner-history-list">
              {entries.map((entry) => (
                <li key={entry.id} className="cleaner-history-row">
                  <span className="cleaner-history-dot" style={{
                    background: HISTORY_STATUS_COLOR[entry.status] ?? "var(--text-secondary)",
                  }} aria-hidden="true" />
                  <span className="cleaner-history-label">{entry.label}</span>
                  <span className="cleaner-history-status text-secondary">
                    {HISTORY_STATUS[entry.status]
                      ? t(HISTORY_STATUS[entry.status] as "cleaner.history.title")
                      : entry.status}
                    {" · "}
                    {formatDate(entry.createdAt)}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
