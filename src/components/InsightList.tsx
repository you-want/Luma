import { useEffect, useState } from "react";
import {
  Archive,
  ChevronRight,
  Clock3,
  Code2,
  Package,
  PackageSearch,
} from "lucide-react";
import { listInsightFiles, revealPath } from "../lib/tauri";
import { categoryColor, categoryIcon } from "../lib/categories";
import { formatBytes, formatDate, formatNumber } from "../lib/format";
import type { FileEntry, InsightKind, InsightSummary } from "../types/scan";

const insightMeta: Record<
  InsightKind,
  { icon: typeof PackageSearch; title: string; color: string }
> = {
  largeFiles: { icon: PackageSearch, title: "超大文件", color: "#FF9F0A" },
  staleFiles: { icon: Clock3, title: "长期未修改", color: "#8E8E93" },
  development: { icon: Code2, title: "开发构建内容", color: "#5E5CE6" },
  archives: { icon: Archive, title: "压缩包", color: "#BF5AF2" },
  installers: { icon: Package, title: "应用与安装包", color: "#64D2FF" },
};

type InsightListProps = {
  scanId?: string;
  insights: InsightSummary[];
  largeFileThreshold: number;
  staleDays: number;
  onSettingsChange: (largeFileThreshold: number, staleDays: number) => void;
};

type FileState =
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "ready"; files: FileEntry[] };

export function InsightList({
  scanId,
  insights,
  largeFileThreshold,
  staleDays,
  onSettingsChange,
}: InsightListProps) {
  const [openKind, setOpenKind] = useState<InsightKind | null>(null);
  const [files, setFiles] = useState<Partial<Record<InsightKind, FileState>>>({});

  // The detail rows depend on the same thresholds as the summary counts, so a
  // rule change invalidates anything already fetched.
  useEffect(() => {
    setOpenKind(null);
    setFiles({});
  }, [scanId, largeFileThreshold, staleDays]);

  async function toggle(kind: InsightKind) {
    if (openKind === kind) {
      setOpenKind(null);
      return;
    }
    setOpenKind(kind);
    if (!scanId || files[kind]?.status === "ready") return;

    setFiles((prev) => ({ ...prev, [kind]: { status: "loading" } }));
    try {
      const loaded = await listInsightFiles(
        scanId,
        kind,
        largeFileThreshold,
        staleDays,
      );
      setFiles((prev) => ({ ...prev, [kind]: { status: "ready", files: loaded } }));
    } catch (error) {
      setFiles((prev) => ({
        ...prev,
        [kind]: {
          status: "error",
          message: error instanceof Error ? error.message : "读取文件明细失败。",
        },
      }));
    }
  }

  return (
    <section className="result-section insight-section" aria-labelledby="insight-title">
      <div className="section-heading compact-heading">
        <h2 id="insight-title">值得留意</h2>
      </div>
      <div className="insight-filters" aria-label="发现规则">
        <label>
          大文件
          <select
            value={largeFileThreshold}
            onChange={(event) => onSettingsChange(Number(event.target.value), staleDays)}
          >
            <option value={256 * 1024 ** 2}>超过 256 MB</option>
            <option value={1024 ** 3}>超过 1 GB</option>
            <option value={5 * 1024 ** 3}>超过 5 GB</option>
          </select>
        </label>
        <label>
          未修改
          <select
            value={staleDays}
            onChange={(event) => onSettingsChange(largeFileThreshold, Number(event.target.value))}
          >
            <option value={90}>超过 90 天</option>
            <option value={180}>超过 180 天</option>
            <option value={365}>超过 1 年</option>
          </select>
        </label>
      </div>
      {insights.length ? (
        <div className="insight-list">
          {insights.map((insight) => {
            const { icon: Icon, title, color } = insightMeta[insight.kind];
            const expanded = openKind === insight.kind;
            const detail = files[insight.kind];
            return (
              <div className="insight" key={insight.kind}>
                <button
                  className="insight-head"
                  type="button"
                  aria-expanded={expanded}
                  onClick={() => toggle(insight.kind)}
                >
                  <span
                    className="insight-icon"
                    style={{ color, background: `${color}1f` }}
                  >
                    <Icon size={16} />
                  </span>
                  <div className="insight-copy">
                    <strong>{title}</strong>
                    <p>
                      {formatNumber(insight.fileCount)} 个文件，合计{" "}
                      {formatBytes(insight.sizeBytes)}。{insight.basis}
                    </p>
                  </div>
                  <ChevronRight
                    className={`insight-chevron${expanded ? " is-open" : ""}`}
                    size={16}
                  />
                </button>
                {expanded && (
                  <div className="insight-files">
                    {detail?.status === "loading" && (
                      <p className="empty-inline">正在读取文件明细...</p>
                    )}
                    {detail?.status === "error" && (
                      <p className="empty-inline">{detail.message}</p>
                    )}
                    {detail?.status === "ready" &&
                      (detail.files.length ? (
                        <>
                          <div className="file-list">
                            {detail.files.map((file) => {
                              const FileIcon = categoryIcon(file.category);
                              const tint = categoryColor(file.category);
                              return (
                                <div className="file-row" key={file.path}>
                                  <span
                                    className="file-icon"
                                    style={{ color: tint, background: `${tint}1f` }}
                                  >
                                    <FileIcon size={16} />
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
                                    onClick={() =>
                                      void revealPath(file.path).catch(() => undefined)
                                    }
                                  >
                                    <ChevronRight size={16} />
                                  </button>
                                </div>
                              );
                            })}
                          </div>
                          {insight.fileCount > detail.files.length && (
                            <p className="insight-more">
                              仅显示最大的 {detail.files.length} 个，共{" "}
                              {formatNumber(insight.fileCount)} 个。
                            </p>
                          )}
                        </>
                      ) : (
                        <p className="empty-inline">没有可列出的文件。</p>
                      ))}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      ) : (
        <p className="empty-inline">当前规则下没有需要特别留意的项目。</p>
      )}
      <p className="insight-note">修改时间不等于最近使用时间；所有发现仅供复查，不代表文件可以安全删除。</p>
    </section>
  );
}
