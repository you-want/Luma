import { useTranslation } from "react-i18next";
import { ChevronRight } from "lucide-react";
import { categoryColor, categoryIcon } from "../lib/categories";
import { formatBytes, formatDate } from "../lib/format";
import type { FileEntry } from "../types/scan";

export function LargeFiles({
  files,
  onReveal,
}: {
  files: FileEntry[];
  onReveal: (path: string) => void;
}) {
  const { t } = useTranslation();
  return (
    <section className="result-section" aria-labelledby="large-files-title">
      <div className="section-heading compact-heading">
        <h2 id="large-files-title">{t("largeFiles.title")}</h2>
        <span>{t("largeFiles.topN", { count: files.length })}</span>
      </div>
      {files.length ? (
        <div className="file-list">
          {files.map((file) => {
            const Icon = categoryIcon(file.category);
            const tint = categoryColor(file.category);
            return (
              <div className="file-row" key={file.path}>
                <span
                  className="file-icon"
                  style={{ color: tint, background: `${tint}1f` }}
                >
                  <Icon size={16} />
                </span>
                <div className="file-copy">
                  <strong title={file.name}>{file.name}</strong>
                  <span title={file.path}>{file.path}</span>
                </div>
                <div className="file-meta">
                  <strong>{formatBytes(file.sizeBytes)}</strong>
                  <span>{formatDate(file.modifiedAt)}</span>
                </div>
                <button
                  className="icon-button"
                  type="button"
                  title={t("common.reveal")}
                  aria-label={t("common.revealNamed", { name: file.name })}
                  onClick={() => onReveal(file.path)}
                >
                  <ChevronRight size={16} />
                </button>
              </div>
            );
          })}
        </div>
      ) : (
        <p className="empty-inline">{t("largeFiles.empty")}</p>
      )}
    </section>
  );
}
