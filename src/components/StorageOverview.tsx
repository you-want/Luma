import { Files, FolderTree, HardDrive, TriangleAlert } from "lucide-react";
import { formatBytes, formatDate, formatNumber } from "../lib/format";
import type { ScanSummary } from "../types/scan";

export function StorageOverview({ summary }: { summary: ScanSummary }) {
  const items = [
    { label: "总占用", value: formatBytes(summary.totalBytes), icon: HardDrive },
    { label: "文件", value: formatNumber(summary.totalFiles), icon: Files },
    { label: "目录", value: formatNumber(summary.totalDirectories), icon: FolderTree },
    { label: "读取错误", value: formatNumber(summary.errorCount), icon: TriangleAlert },
  ];

  return (
    <section aria-labelledby="overview-title">
      <div className="section-heading result-heading">
        <h2 id="overview-title">{summary.rootPath}</h2>
        <span className="scan-time">完成于 {formatDate(summary.finishedAt)}</span>
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
