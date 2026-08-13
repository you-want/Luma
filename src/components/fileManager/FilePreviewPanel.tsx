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
import type { FileEntry } from "../../types/scan";
import { formatBytes, formatDate } from "../../lib/format";

// ── Preview strategies ─────────────────────────────────────────────────────────

type PreviewKind = "image" | "text" | "pdf" | "none";

const IMAGE_EXTS = new Set(["jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "ico", "tiff"]);
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
  if (TEXT_EXTS.has(ext) || file.category === "code" || file.category === "documents") {
    return "text";
  }
  return "none";
}

// ── Image preview ──────────────────────────────────────────────────────────────

function ImagePreview({ path }: { path: string }) {
  // Use Tauri's asset protocol to serve local files safely.
  // Convert the stored canonical path to an asset:// URL.
  const assetUrl = `asset://localhost/${encodeURIComponent(path.replace(/^\//, ""))}`;
  return (
    <div className="fm-preview-image-wrap">
      <img
        src={assetUrl}
        alt=""
        className="fm-preview-image"
        onError={(e) => {
          (e.currentTarget as HTMLImageElement).style.display = "none";
        }}
      />
    </div>
  );
}

// ── Text / code preview ────────────────────────────────────────────────────────

type TextState = { status: "loading" } | { status: "ok"; text: string } | { status: "error" };

function TextPreview({ file }: { file: FileEntry }) {
  const [state, setState] = useState<TextState>({ status: "loading" });

  useEffect(() => {
    setState({ status: "loading" });
    // Rust command reads first 64 KB and returns a string.
    invoke<string>("read_text_preview", { path: file.path })
      .then((text) => setState({ status: "ok", text }))
      .catch(() => setState({ status: "error" }));
  }, [file.path]);

  if (state.status === "loading") {
    return <div className="fm-preview-loading">读取中…</div>;
  }
  if (state.status === "error") {
    return <div className="fm-preview-error">无法读取文件内容。</div>;
  }
  return (
    <pre className="fm-preview-text">
      <code>{state.text}</code>
    </pre>
  );
}

// ── PDF preview ────────────────────────────────────────────────────────────────

function PdfPreview({ path }: { path: string }) {
  // macOS WebKit renders PDFs natively inside an iframe with a file:// URL.
  return (
    <iframe
      className="fm-preview-pdf"
      src={`file://${path}`}
      title="PDF 预览"
      sandbox=""
    />
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
  file: FileEntry | null;
  onClose: () => void;
  onReveal: (path: string) => void;
  onOpen: (path: string) => void;
};

export function FilePreviewPanel({ file, onClose, onReveal, onOpen }: Props) {
  if (!file) {
    return (
      <aside className="fm-preview fm-preview--empty">
        <div className="fm-preview-placeholder">
          <File size={28} />
          <p>选中文件查看预览</p>
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
          aria-label="关闭预览"
        >
          <X size={14} />
        </button>
      </div>

      {/* Inline preview */}
      {kind === "image" && <ImagePreview path={file.path} />}
      {kind === "text"  && <TextPreview file={file} />}
      {kind === "pdf"   && <PdfPreview path={file.path} />}

      {/* Metadata */}
      <dl className="fm-preview-meta">
        <MetaRow label="路径" value={file.path} />
        <MetaRow label="分类" value={file.category} />
        <MetaRow label="大小" value={formatBytes(file.sizeBytes)} />
        <MetaRow label="修改时间" value={formatDate(file.modifiedAt)} />
        {file.extension && <MetaRow label="扩展名" value={`.${file.extension}`} />}
        {file.contentHash && (
          <MetaRow label="内容哈希" value={file.contentHash.slice(0, 16) + "…"} />
        )}
      </dl>

      {/* Actions */}
      <div className="fm-preview-actions">
        <button
          type="button"
          className="button button-primary"
          onClick={() => onOpen(file.path)}
        >
          打开
        </button>
        <button
          type="button"
          className="button"
          onClick={() => onReveal(file.path)}
        >
          在 Finder 中显示
        </button>
      </div>
    </aside>
  );
}
