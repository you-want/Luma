import { useTranslation } from "react-i18next";
import { Files, FolderTree, HardDrive, TriangleAlert } from "lucide-react";
import { formatBytes, formatDate, formatNumber } from "../lib/format";
import type { ScanSummary } from "../types/scan";

export function StorageOverview({ summary }: { summary: ScanSummary }) {
  const { t } = useTranslation();
  const items = [
    { label: t("overview.totalSize"), value: formatBytes(summary.totalBytes), icon: HardDrive },
    { label: t("overview.files"), value: formatNumber(summary.totalFiles), icon: Files },
    { label: t("overview.directories"), value: formatNumber(summary.totalDirectories), icon: FolderTree },
    { label: t("overview.readErrors"), value: formatNumber(summary.errorCount), icon: TriangleAlert },
  ];

  return (
    <section aria-labelledby="overview-title">
      <div className="section-heading result-heading">
        <h2 id="overview-title">{summary.rootPath}</h2>
        <span className="scan-time">{t("overview.finishedAt", { time: formatDate(summary.finishedAt) })}</span>
      </div>
      <div className="metric-grid">
        {items.map(({ label, value, icon: Icon }) => (
          <article className="metric" key={label}>
            <Icon size={16} />
            <span>{label}</span>
            <strong>{value}</strong>
          </article>
        ))}
      </div>
    </section>
  );
}
