// File preview panel — shows metadata for all files,
// inline preview for images / text / code / PDF.

import { useEffect, useState } from "react";
import {
  File,
  FileImage,
  FileText,
  FileVideo,
  FileAudio,
  FileArchive,
  FileCode,
  X,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import type { FileEntry } from "../../types/scan";
import { categoryLabelKey } from "../../lib/categories";
import { errorMessage } from "../../lib/errors";
import { formatBytes, formatDate } from "../../lib/format";
import { PdfPreview } from "./PdfPreview";
import { ButtonSpinner } from "../ButtonSpinner";
import type { FileOperation } from "./useFileOps";

// ── Preview strategies ─────────────────────────────────────────────────────────

type PreviewKind = "image" | "text" | "pdf" | "none";

const IMAGE_EXTS = new Set(["jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "ico", "tiff", "tif"]);
const TEXT_EXTS  = new Set([
  "txt", "md", "markdown", "json", "yaml", "yml", "toml", "xml",
  "csv", "log", "env", "sh", "bash", "zsh", "fish",
  "js", "ts", "jsx", "tsx", "css", "html", "htm", "vue", "svelte",
  "rs", "py", "go", "java", "kt", "swift", "c", "cpp", "h", "hpp",
  "rb", "php", "lua", "sql", "graphql", "proto",
]);

function previewKindFor(file: FileEntry): PreviewKind {
  const ext = (file.extension ?? "").toLowerCase();
  if (IMAGE_EXTS.has(ext)) return "image";
  if (ext === "pdf") return "pdf";
  if (TEXT_EXTS.has(ext) || file.category === "code") {
    return "text";
  }
  return "none";
}

// Local binary files are delivered explicitly over IPC, then rendered as
// short-lived blob URLs. No file:// or unconfigured asset protocol is needed.
type BinaryState = { status: "loading" } | { status: "ok"; url: string } | { status: "error"; message: string };
function BinaryFilePreview({ file, scanId, kind }: { file: FileEntry; scanId: string; kind: "image" | "pdf" }) {
  const { t } = useTranslation();
  const [state, setState] = useState<BinaryState>({ status: "loading" });
  useEffect(() => {
    let disposed = false;
    let url: string | undefined;
    invoke<{ mimeType: string; data: string }>("read_binary_preview", { scanId, path: file.path }).then(result => {
      if (disposed) return;
      const bytes = Uint8Array.from(atob(result.data), character => character.charCodeAt(0));
      url = URL.createObjectURL(new Blob([bytes], { type: result.mimeType }));
      setState({ status: "ok", url });
    }).catch(error => {
      if (!disposed) setState({ status: "error", message: errorMessage(error) });
    });
    return () => { disposed = true; if (url) URL.revokeObjectURL(url); };
  }, [scanId, file.path]);
  if (state.status === "loading") return <div className="fm-preview-loading">{t("fileManager.preview.loading")}</div>;
  if (state.status === "error") return <div className="fm-preview-error" role="status">{state.message}</div>;
  if (kind === "pdf") return <PdfPreview url={state.url} />;
  return <div className="fm-preview-image-wrap"><img src={state.url} alt={file.name} className="fm-preview-image"
    onError={() => setState({ status: "error", message: t("fileManager.preview.readError") })} /></div>;
}

// ── Text / code preview ────────────────────────────────────────────────────────

type TextState = { status: "loading" } | { status: "ok"; text: string } | { status: "error" };

function TextPreview({ file }: { file: FileEntry }) {
  const { t } = useTranslation();
  const [state, setState] = useState<TextState>({ status: "loading" });

  useEffect(() => {
    let disposed = false;
    setState({ status: "loading" });
    // Rust command reads first 64 KB and returns a string.
    invoke<string>("read_text_preview", { path: file.path })
      .then((text) => { if (!disposed) setState({ status: "ok", text }); })
      .catch(() => { if (!disposed) setState({ status: "error" }); });
    return () => { disposed = true; };
  }, [file.path]);

  if (state.status === "loading") {
    return <div className="fm-preview-loading">{t("fileManager.preview.loading")}</div>;
  }
  if (state.status === "error") {
    return <div className="fm-preview-error">{t("fileManager.preview.readError")}</div>;
  }
  return (
    <pre className="fm-preview-text">
      <code>{state.text}</code>
    </pre>
  );
}

// ── Metadata table ─────────────────────────────────────────────────────────────

function MetaRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="fm-preview-meta-row">
      <dt>{label}</dt>
      <dd title={value}>{value}</dd>
    </div>
  );
}

function CategoryIcon({ category }: { category: string }) {
  const size = 32;
  switch (category) {
    case "images":    return <FileImage size={size} />;
    case "videos":    return <FileVideo size={size} />;
    case "audio":     return <FileAudio size={size} />;
    case "documents": return <FileText size={size} />;
    case "code":      return <FileCode size={size} />;
    case "archives":  return <FileArchive size={size} />;
    default:          return <File size={size} />;
  }
}

// ── Root export ────────────────────────────────────────────────────────────────

type Props = {
  pendingAction?: FileOperation | "open" | "reveal" | null;
  scanId: string;
  file: FileEntry | null;
  onClose: () => void;
  onReveal: (path: string) => void;
  onOpen: (path: string) => void;
};

export function FilePreviewPanel({ scanId, file, onClose, onReveal, onOpen, pendingAction = null }: Props) {
  const { t } = useTranslation();
  if (!file) {
    return (
      <aside className="fm-preview fm-preview--empty">
        <div className="fm-preview-placeholder">
          <File size={28} />
          <p>{t("fileManager.preview.emptyHint")}</p>
        </div>
      </aside>
    );
  }

  const kind = previewKindFor(file);

  return (
    <aside className="fm-preview">
      <div className="fm-preview-header">
        <span className={`fm-preview-icon fm-file-icon--${file.category}`}>
          <CategoryIcon category={file.category} />
        </span>
        <div className="fm-preview-title">
          <strong title={file.name}>{file.name}</strong>
          <span>{formatBytes(file.sizeBytes)}</span>
        </div>
        <button
          type="button"
          className="icon-button fm-preview-close"
          onClick={onClose}
          aria-label={t("fileManager.preview.close")}
        >
          <X size={14} />
        </button>
      </div>

      {/* Inline preview */}
      {(kind === "image" || kind === "pdf") && <BinaryFilePreview key={file.path} file={file} scanId={scanId} kind={kind} />}
      {kind === "text" && <TextPreview key={file.path} file={file} />}
      {kind === "none" && <p className="fm-preview-error">{t("fileManager.preview.openHint")}</p>}

      {/* Metadata */}
      <dl className="fm-preview-meta">
        <MetaRow label={t("fileManager.preview.meta.path")} value={file.path} />
        <MetaRow label={t("fileManager.preview.meta.category")} value={t(categoryLabelKey(file.category))} />
        <MetaRow label={t("fileManager.preview.meta.size")} value={formatBytes(file.sizeBytes)} />
        <MetaRow label={t("fileManager.preview.meta.modified")} value={formatDate(file.modifiedAt)} />
        {file.extension && <MetaRow label={t("fileManager.preview.meta.extension")} value={`.${file.extension}`} />}
        {file.contentHash && (
          <MetaRow label={t("fileManager.preview.meta.hash")} value={file.contentHash.slice(0, 16) + "…"} />
        )}
      </dl>

      {/* Actions */}
      <div className="fm-preview-actions">
        <button
          type="button"
          className="button button-primary"
          onClick={() => onOpen(file.path)}
          disabled={pendingAction !== null}
          aria-busy={pendingAction === "open"}
        >
          {pendingAction === "open" && <ButtonSpinner />}
          {t("fileManager.preview.open")}
        </button>
        <button
          type="button"
          className="button"
          title={t("common.reveal")}
          onClick={() => onReveal(file.path)}
          disabled={pendingAction !== null}
          aria-busy={pendingAction === "reveal"}
        >
          {pendingAction === "reveal" && <ButtonSpinner />}
          {t("fileManager.preview.reveal")}
        </button>
      </div>
    </aside>
  );
}
