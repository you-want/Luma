import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowRight, GitCompareArrows } from "lucide-react";
import { compareScans, listScanHistory } from "../lib/tauri";
import {
  categoryColor,
  categoryIcon,
  categoryLabelKey,
  categoryTint,
} from "../lib/categories";
import { errorMessage } from "../lib/errors";
import { formatBytes, formatDate, formatNumber } from "../lib/format";
import type { ScanComparison, ScanSummary } from "../types/scan";

type ScanHistoryProps = {
  scanId: string;
};

type CompareState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "ready"; comparison: ScanComparison };

// A signed byte total, prefixed with + / − so growth and reclaimed space read
// at a glance. Zero is rendered without a sign.
function formatSignedBytes(delta: number): string {
  if (delta === 0) return "±0 B";
  const sign = delta > 0 ? "+" : "−";
  return `${sign}${formatBytes(Math.abs(delta))}`;
}

function formatSignedCount(delta: number): string {
  if (delta === 0) return "±0";
  const sign = delta > 0 ? "+" : "−";
  return `${sign}${formatNumber(Math.abs(delta))}`;
}

function deltaClass(delta: number): string {
  if (delta > 0) return "delta-up";
  if (delta < 0) return "delta-down";
  return "delta-flat";
}

export function ScanHistory({ scanId }: ScanHistoryProps) {
  const { t } = useTranslation();
  const [history, setHistory] = useState<ScanSummary[] | null>(null);
  const [baseId, setBaseId] = useState<string | null>(null);
  const [compare, setCompare] = useState<CompareState>({ status: "idle" });

  useEffect(() => {
    let disposed = false;
    setHistory(null);
    setBaseId(null);
    setCompare({ status: "idle" });
    listScanHistory(scanId)
      .then((runs) => {
        if (disposed) return;
        setHistory(runs);
        // Default the comparison base to the most recent *earlier* run of the
        // same directory, i.e. the first entry that is not the current scan.
        const earlier = runs.find((run) => run.scanId !== scanId);
        if (earlier) setBaseId(earlier.scanId);
      })
      .catch(() => {
        if (!disposed) setHistory([]);
      });
    return () => {
      disposed = true;
    };
  }, [scanId]);

  useEffect(() => {
    if (!baseId || baseId === scanId) {
      setCompare({ status: "idle" });
      return;
    }
    let disposed = false;
    setCompare({ status: "loading" });
    compareScans(baseId, scanId)
      .then((comparison) => {
        if (!disposed) setCompare({ status: "ready", comparison });
      })
      .catch((error) => {
        if (disposed) return;
        setCompare({
          status: "error",
          message: errorMessage(error, t("history.compareError")),
        });
      });
    return () => {
      disposed = true;
    };
  }, [baseId, scanId, t]);

  // Earlier runs the user can compare the current scan against.
  const earlierRuns = useMemo(
    () => (history ?? []).filter((run) => run.scanId !== scanId),
    [history, scanId],
  );

  if (history === null) {
    return (
      <section className="result-section" aria-labelledby="history-title">
        <div className="section-heading compact-heading">
          <h2 id="history-title">{t("history.title")}</h2>
        </div>
        <p className="empty-inline">{t("history.loading")}</p>
      </section>
    );
  }

  if (earlierRuns.length === 0) {
    return (
      <section className="result-section" aria-labelledby="history-title">
        <div className="section-heading compact-heading">
          <h2 id="history-title">{t("history.title")}</h2>
        </div>
        <p className="empty-inline">{t("history.noEarlier")}</p>
      </section>
    );
  }

  return (
    <section className="result-section" aria-labelledby="history-title">
      <div className="section-heading compact-heading">
        <h2 id="history-title">{t("history.title")}</h2>
        <span>{t("history.recentN", { count: history.length })}</span>
      </div>

      <label className="history-picker">
        {t("history.base")}
        <select
          value={baseId ?? ""}
          onChange={(event) => setBaseId(event.target.value || null)}
        >
          {earlierRuns.map((run) => (
            <option key={run.scanId} value={run.scanId}>
              {formatDate(run.finishedAt ?? run.startedAt)} · {formatBytes(run.totalBytes)}
            </option>
          ))}
        </select>
      </label>

      {compare.status === "loading" && (
        <p className="empty-inline">{t("history.comparing")}</p>
      )}
      {compare.status === "error" && (
        <p className="empty-inline">{compare.message}</p>
      )}
      {compare.status === "ready" && (
        <ComparisonView comparison={compare.comparison} />
      )}
    </section>
  );
}

function ComparisonView({ comparison }: { comparison: ScanComparison }) {
  const { t } = useTranslation();
  const { base, target, totalBytesDelta, totalFilesDelta, categories } = comparison;
  // Only categories that actually moved are worth showing.
  const changed = categories.filter(
    (category) => category.sizeDelta !== 0 || category.fileCountDelta !== 0,
  );

  return (
    <div className="history-compare">
      <div className="history-range">
        <span>{formatDate(base.finishedAt ?? base.startedAt)}</span>
        <ArrowRight size={14} />
        <span>{formatDate(target.finishedAt ?? target.startedAt)}</span>
      </div>

      <div className="history-totals">
        <div className="history-total">
          <span>{t("history.totalChange")}</span>
          <strong className={deltaClass(totalBytesDelta)}>
            {formatSignedBytes(totalBytesDelta)}
          </strong>
          <span className="history-total-abs">
            {formatBytes(base.totalBytes)} → {formatBytes(target.totalBytes)}
          </span>
        </div>
        <div className="history-total">
          <span>{t("history.fileChange")}</span>
          <strong className={deltaClass(totalFilesDelta)}>
            {formatSignedCount(totalFilesDelta)}
          </strong>
          <span className="history-total-abs">
            {formatNumber(base.totalFiles)} → {formatNumber(target.totalFiles)}
          </span>
        </div>
      </div>

      {changed.length ? (
        <div className="history-categories">
          {changed.map((category) => {
            const Icon = categoryIcon(category.category);
            const tint = categoryColor(category.category);
            return (
              <div className="history-category" key={category.category}>
                <span
                  className="file-icon"
                  style={{
                    color: tint,
                    background: categoryTint(category.category),
                  }}
                >
                  <Icon size={16} />
                </span>
                <div className="file-copy">
                  <strong>{t(categoryLabelKey(category.category))}</strong>
                  <span>
                    {formatBytes(category.baseSizeBytes)} →{" "}
                    {formatBytes(category.targetSizeBytes)}
                  </span>
                </div>
                <strong
                  className={`history-delta ${deltaClass(category.sizeDelta)}`}
                >
                  {formatSignedBytes(category.sizeDelta)}
                </strong>
              </div>
            );
          })}
        </div>
      ) : (
        <p className="empty-inline">
          <GitCompareArrows size={14} /> {t("history.noChange")}
        </p>
      )}
      <p className="insight-note">{t("history.note")}</p>
    </div>
  );
}
