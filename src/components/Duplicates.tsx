import { useEffect, useState } from "react";
import { Copy, ChevronRight, AlertCircle } from "lucide-react";
import { findDuplicates, revealPath } from "../lib/tauri";
import { categoryColor, categoryIcon } from "../lib/categories";
import { formatBytes, formatDate, formatNumber } from "../lib/format";
import type { DuplicateGroup, FileEntry } from "../types/scan";

type DuplicatesProps = {
  scanId: string;
};

type DuplicateState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "ready"; groups: DuplicateGroup[] };

export function Duplicates({ scanId }: DuplicatesProps) {
  const [state, setState] = useState<DuplicateState>({ status: "idle" });
  const [expandedHash, setExpandedHash] = useState<string | null>(null);
  const [minSize, setMinSize] = useState(1024 * 1024); // 默认 1MB

  useEffect(() => {
    setState({ status: "idle" });
    setExpandedHash(null);
  }, [scanId]);

  async function handleFind() {
    setState({ status: "loading" });
    try {
      const groups = await findDuplicates(scanId, minSize);
      setState({ status: "ready", groups });
    } catch (error) {
      setState({
        status: "error",
        message: error instanceof Error ? error.message : "查找重复文件失败。",
      });
    }
  }

  function toggleGroup(hash: string) {
    setExpandedHash(expandedHash === hash ? null : hash);
  }

  return (
    <section className="result-section duplicates-section" aria-labelledby="duplicates-title">
      <div className="section-heading compact-heading">
        <h2 id="duplicates-title">重复文件候选</h2>
      </div>

      <div className="duplicates-controls">
        <label>
          最小文件大小
          <select value={minSize} onChange={(e) => setMinSize(Number(e.target.value))}>
            <option value={1024 * 1024}>1 MB</option>
            <option value={10 * 1024 * 1024}>10 MB</option>
            <option value={100 * 1024 * 1024}>100 MB</option>
            <option value={1024 * 1024 * 1024}>1 GB</option>
          </select>
        </label>
        <button
          type="button"
          className="primary-button"
          onClick={handleFind}
          disabled={state.status === "loading"}
        >
          {state.status === "loading" ? "正在查找..." : "查找重复"}
        </button>
      </div>

      {state.status === "loading" && (
        <p className="empty-inline">正在分析文件内容，可能需要一段时间...</p>
      )}

      {state.status === "error" && (
        <div className="status-message status-error" role="alert">
          <AlertCircle size={20} />
          <div>
            <strong>查找失败</strong>
            <p>{state.message}</p>
          </div>
        </div>
      )}

      {state.status === "ready" &&
        (state.groups.length > 0 ? (
          <>
            <div className="duplicates-summary">
              <p>
                找到 <strong>{state.groups.length}</strong> 组重复文件，可节省约{" "}
                <strong>
                  {formatBytes(state.groups.reduce((sum, g) => sum + g.wastedBytes, 0))}
                </strong>{" "}
                空间。
              </p>
            </div>
            <div className="duplicate-list">
              {state.groups.map((group) => {
                const expanded = expandedHash === group.contentHash;
                return (
                  <div className="duplicate-group" key={group.contentHash}>
                    <button
                      className="duplicate-head"
                      type="button"
                      aria-expanded={expanded}
                      onClick={() => toggleGroup(group.contentHash)}
                    >
                      <span className="duplicate-icon" style={{ color: "#FF9F0A" }}>
                        <Copy size={16} />
                      </span>
                      <div className="duplicate-copy">
                        <strong>
                          {formatNumber(group.fileCount)} 个相同文件 · 每个{" "}
                          {formatBytes(group.sizeBytes)}
                        </strong>
                        <p>可节省 {formatBytes(group.wastedBytes)}</p>
                      </div>
                      <ChevronRight
                        className={`duplicate-chevron${expanded ? " is-open" : ""}`}
                        size={16}
                      />
                    </button>
                    {expanded && (
                      <div className="duplicate-files">
                        <div className="file-list">
                          {group.files.map((file) => (
                            <DuplicateFileRow key={file.path} file={file} />
                          ))}
                        </div>
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          </>
        ) : (
          <p className="empty-inline">在当前条件下未找到重复文件。</p>
        ))}

      <p className="insight-note">
        内容完全相同的文件才会被识别为重复。删除前请确认文件不再需要，某些应用或系统依赖可能需要保留。
      </p>
    </section>
  );
}

function DuplicateFileRow({ file }: { file: FileEntry }) {
  const FileIcon = categoryIcon(file.category);
  const tint = categoryColor(file.category);

  return (
    <div className="file-row">
      <span className="file-icon" style={{ color: tint, background: `${tint}1f` }}>
        <FileIcon size={16} />
      </span>
      <div className="file-copy">
        <strong title={file.name}>{file.name}</strong>
        <span title={file.path}>{file.path}</span>
      </div>
      <div className="file-meta">
        <strong>{formatBytes(file.sizeBytes)}</strong>
        <span>{file.modifiedAt ? formatDate(file.modifiedAt) : "—"}</span>
      </div>
      <button
        className="icon-button"
        type="button"
        title="在 Finder 中显示"
        aria-label={`在 Finder 中显示 ${file.name}`}
        onClick={() => void revealPath(file.path).catch(() => undefined)}
      >
        <ChevronRight size={16} />
      </button>
    </div>
  );
}
