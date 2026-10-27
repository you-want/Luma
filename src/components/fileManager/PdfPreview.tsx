import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { PDFDocumentLoadingTask, RenderTask } from "pdfjs-dist";

// Render a bounded first-page thumbnail without relying on a WebView PDF plugin.
// The parser and worker are loaded only when a PDF is selected.
export function PdfPreview({ url }: { url: string }) {
  const { t } = useTranslation();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [status, setStatus] = useState<"loading" | "ready" | "error">("loading");

  useEffect(() => {
    let disposed = false;
    let loading: PDFDocumentLoadingTask | undefined;
    let render: RenderTask | undefined;
    setStatus("loading");
    async function draw() {
      try {
        const [pdfjs, worker] = await Promise.all([
          import("pdfjs-dist"),
          import("pdfjs-dist/build/pdf.worker.min.mjs?url"),
        ]);
        if (disposed) return;
        pdfjs.GlobalWorkerOptions.workerSrc = worker.default;
        loading = pdfjs.getDocument({ url, useSystemFonts: true, enableXfa: false });
        const document = await loading.promise;
        if (disposed) return;
        const page = await document.getPage(1);
        if (disposed || !canvasRef.current) return;
        const original = page.getViewport({ scale: 1 });
        const viewport = page.getViewport({ scale: Math.min(2, 1000 / original.width, 1400 / original.height) });
        const canvas = canvasRef.current;
        canvas.width = Math.ceil(viewport.width);
        canvas.height = Math.ceil(viewport.height);
        render = page.render({ canvas, viewport });
        await render.promise;
        if (!disposed) setStatus("ready");
      } catch (error) {
        if (!disposed) { console.warn("PDF preview unavailable", error); setStatus("error"); }
      }
    }
    void draw();
    return () => { disposed = true; render?.cancel(); void loading?.destroy(); };
  }, [url]);

  return <div className="fm-preview-pdf" aria-busy={status === "loading"}>
    {status === "loading" && <p className="fm-preview-loading">{t("fileManager.preview.loading")}</p>}
    {status === "error" && <p className="fm-preview-error" role="status">{t("fileManager.preview.readError")} {t("fileManager.preview.openHint")}</p>}
    <canvas ref={canvasRef} role="img" aria-label={t("fileManager.preview.pdfTitle")} hidden={status !== "ready"} />
    {status === "ready" && <p className="fm-preview-pdf-hint">{t("fileManager.preview.pdfHint")}</p>}
  </div>;
}
