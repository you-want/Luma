import { useTranslation } from "react-i18next";
import { LoaderCircle } from "lucide-react";
import { formatBytes, formatNumber } from "../lib/format";
import type { ScanProgress as ScanProgressType } from "../types/scan";

export function ScanProgress({ progress }: { progress?: ScanProgressType }) {
  const { t } = useTranslation();
  return (
    <section className="progress-panel" aria-live="polite">
      <div className="progress-title">
        <LoaderCircle className="spin" size={22} />
        <div>
          <strong>{t("progress.title")}</strong>
          <span title={progress?.currentPath}>{progress?.currentPath || t("progress.reading")}</span>
        </div>
      </div>
      <div className="progress-track" aria-hidden="true">
        <span />
      </div>
      <dl className="progress-stats">
        <div><dt>{t("progress.files")}</dt><dd>{formatNumber(progress?.filesScanned ?? 0)}</dd></div>
        <div><dt>{t("progress.directories")}</dt><dd>{formatNumber(progress?.directoriesScanned ?? 0)}</dd></div>
        <div><dt>{t("progress.read")}</dt><dd>{formatBytes(progress?.bytesScanned ?? 0)}</dd></div>
        <div><dt>{t("progress.skippedErrors")}</dt><dd>{formatNumber(progress?.errors ?? 0)}</dd></div>
      </dl>
    </section>
  );
}
