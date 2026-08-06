import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Copy, ChevronRight, AlertCircle } from "lucide-react";
import { findDuplicates, revealPath } from "../lib/tauri";
import { categoryColor, categoryIcon, categoryTint } from "../lib/categories";
import { formatBytes, formatDate, formatNumber } from "../lib/format";
import { errorMessage } from "../lib/errors";
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
  const { t } = useTranslation();
  const [state, setState] = useState<DuplicateState>({ status: "idle" });
  const [expandedHash, setExpandedHash] = useState<string | null>(null);
  const [minSize, setMinSize] = useState(1024 * 1024); // default 1MB

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
        message: errorMessage(error, t("duplicates.findError")),
      });
    }
  }

  function toggleGroup(hash: string) {
    setExpandedHash(expandedHash === hash ? null : hash);
  }

  return (
    <section className="result-section duplicates-section" aria-labelledby="duplicates-title">
      <div className="section-heading compact-heading">
        <h2 id="duplicates-title">{t("duplicates.title")}</h2>
      </div>

      <div className="duplicates-controls">
        <label>
          {t("duplicates.minSize")}
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
          {state.status === "loading" ? t("duplicates.finding") : t("duplicates.find")}
        </button>
      </div>

      {state.status === "loading" && (
        <p className="empty-inline">{t("duplicates.findingBody")}</p>
      )}

      {state.status === "error" && (
        <div className="status-message status-error" role="alert">
          <AlertCircle size={20} />
          <div>
            <strong>{t("duplicates.findFailedTitle")}</strong>
            <p>{state.message}</p>
          </div>
        </div>
      )}

      {state.status === "ready" &&
        (state.groups.length > 0 ? (
          <>
            <div className="duplicates-summary">
              <p>
                {t("duplicates.foundSummary", {
                  groups: formatNumber(state.groups.length),
                  size: formatBytes(
                    state.groups.reduce((sum, g) => sum + g.wastedBytes, 0),
                  ),
                })}
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
                      <span
                        className="duplicate-icon"
                        style={{ color: "var(--han-color-warning)" }}
                      >
                        <Copy size={16} />
                      </span>
                      <div className="duplicate-copy">
                        <strong>
                          {t("duplicates.groupSummary", {
                            count: formatNumber(group.fileCount),
                            size: formatBytes(group.sizeBytes),
                          })}
                        </strong>
                        <p>{t("duplicates.canSave", { size: formatBytes(group.wastedBytes) })}</p>
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
          <p className="empty-inline">{t("duplicates.notFound")}</p>
        ))}

      <p className="insight-note">{t("duplicates.note")}</p>
    </section>
  );
}

function DuplicateFileRow({ file }: { file: FileEntry }) {
  const { t } = useTranslation();
  const FileIcon = categoryIcon(file.category);
  const tint = categoryColor(file.category);

  return (
    <div className="file-row">
      <span
        className="file-icon"
        style={{ color: tint, background: categoryTint(file.category) }}
      >
        <FileIcon size={16} />
      </span>
      <div className="file-copy">
        <strong title={file.name}>{file.name}</strong>
        <span title={file.path}>{file.path}</span>
      </div>
      <div className="file-meta">
        <strong>{formatBytes(file.sizeBytes)}</strong>
        <span>{file.modifiedAt ? formatDate(file.modifiedAt) : t("common.missing")}</span>
      </div>
      <button
        className="icon-button"
        type="button"
        title={t("common.reveal")}
        aria-label={t("common.revealNamed", { name: file.name })}
        onClick={() => void revealPath(file.path).catch(() => undefined)}
      >
        <ChevronRight size={16} />
      </button>
    </div>
  );
}
