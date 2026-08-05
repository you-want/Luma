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
  return (
    <section className="result-section" aria-labelledby="large-files-title">
      <div className="section-heading compact-heading">
        <h2 id="large-files-title">最大文件</h2>
        <span>前 {files.length} 项</span>
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
                  title="在 Finder 中显示"
                  aria-label={`在 Finder 中显示 ${file.name}`}
                  onClick={() => onReveal(file.path)}
                >
                  <ChevronRight size={16} />
                </button>
              </div>
            );
          })}
        </div>
      ) : (
        <p className="empty-inline">这个目录里没有可列出的文件。</p>
      )}
    </section>
  );
}
