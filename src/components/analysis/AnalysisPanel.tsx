// AnalysisPanel — the 空间分析 view body. Presentation-only composition of
// the analysis sections; the scan data arrives via props and write intents
// (organize / duplicates routing / per-file trash) are delegated upward.

import { CategoryList } from "./CategoryList";
import { CleanupPanel } from "./CleanupPanel";
import { Duplicates } from "./Duplicates";
import { InsightList } from "./InsightList";
import { LargeFiles } from "./LargeFiles";
import Projects from "./Projects";
import { ScanHistory } from "./ScanHistory";
import Search from "./Search";
import { StorageOverview } from "./StorageOverview";
import { useAppStore } from "../../app/store";
import type { ScanSummary } from "../../types/scan";

type AnalysisPanelProps = {
  summary: ScanSummary;
  onDiscoverySettingsChange: (largeFileThreshold: number, staleDays: number) => void;
  onReveal: (path: string) => void;
  onOrganize: (sourceDir: string) => void;
  onFindDuplicates: () => void;
};

export function AnalysisPanel({
  summary,
  onDiscoverySettingsChange,
  onReveal,
  onOrganize,
  onFindDuplicates,
}: AnalysisPanelProps) {
  const app = useAppStore();

  return (
    <div className="content">
      <div className="results">
        <StorageOverview summary={summary} />
        <div className="results-grid">
          <CategoryList categories={summary.categories} totalBytes={summary.totalBytes} />
          <InsightList
            scanId={summary.scanId}
            insights={app.insights}
            largeFileThreshold={app.largeFileThreshold}
            staleDays={app.staleDays}
            onSettingsChange={onDiscoverySettingsChange}
          />
        </div>
        <LargeFiles files={app.largeFiles} onReveal={onReveal} scanId={summary.scanId} />
        <CleanupPanel scanId={summary.scanId} rootPath={summary.rootPath}
          onOrganize={onOrganize} onFindDuplicates={onFindDuplicates} />
        <Search
          scanId={summary.scanId}
          categories={summary.categories.map((c) => c.category)}
        />
        <Duplicates scanId={summary.scanId} />
        <Projects scanId={summary.scanId} />
        <ScanHistory scanId={summary.scanId} />
      </div>
    </div>
  );
}
