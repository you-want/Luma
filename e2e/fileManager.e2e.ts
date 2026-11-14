import { test, expect, type Page } from "@playwright/test";
import { installTauriMock, type HandlerMap } from "./tauriMock";

// Coverage carried over from the retired scripts/test-file-manager.py
// browser regression: list pagination (205 files), real image/text/PDF
// preview rendering, and a slow-directory-listing race guard. The old
// Python suite targeted the pre-redesign DOM (organize dialog, old button
// names) and could not be maintained alongside the Playwright e2e suite.

const SCAN_SUMMARY = {
  scanId: "scan-1",
  rootPath: "/Users/test/Desktop",
  status: "completed" as const,
  startedAt: 1_700_000_000,
  finishedAt: 1_700_000_050,
  totalFiles: 205,
  totalDirectories: 1,
  totalBytes: 5_000,
  errorCount: 0,
  categories: [{ category: "documents", fileCount: 205, sizeBytes: 5_000 }],
};

const PNG_1PX_BASE64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";

// One real single-page PDF (same structure as the retired Python fixture) so
// pdfjs-dist actually parses and renders page 1 onto the canvas.
function minimalPdfBase64(): string {
  const content = "BT /F1 16 Tf 20 290 Td (Luma PDF preview) Tj ET";
  const objects = [
    "<< /Type /Catalog /Pages 2 0 R >>",
    "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 260 340] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    `<< /Length ${content.length} >>\nstream\n${content}\nendstream`,
  ];
  let pdf = "%PDF-1.4\n";
  const offsets = [0];
  objects.forEach((object, index) => {
    offsets.push(pdf.length);
    pdf += `${index + 1} 0 obj\n${object}\nendobj\n`;
  });
  const xref = pdf.length;
  pdf += `xref\n0 ${offsets.length}\n0000000000 65535 f \n`;
  pdf += offsets.slice(1).map(offset => `${String(offset).padStart(10, "0")} 00000 n \n`).join("");
  pdf += `trailer\n<< /Size ${offsets.length} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return Buffer.from(pdf, "latin1").toString("base64");
}

function pagedFiles(count: number) {
  return Array.from({ length: count }, (_, id) => ({
    id,
    name: `file-${String(id).padStart(3, "0")}.txt`,
    path: `/Users/test/Desktop/file-${String(id).padStart(3, "0")}.txt`,
    category: "documents",
    extension: "txt",
    sizeBytes: 25,
    modifiedAt: 100,
    isHidden: false,
  }));
}

const IMAGE_FILE = {
  id: 900,
  name: "pic.png",
  path: "/Users/test/Desktop/pic.png",
  category: "images",
  extension: "png",
  sizeBytes: 82,
  modifiedAt: 100,
  isHidden: false,
};
const TEXT_FILE = {
  id: 901,
  name: "note.txt",
  path: "/Users/test/Desktop/note.txt",
  category: "documents",
  extension: "txt",
  sizeBytes: 14,
  modifiedAt: 100,
  isHidden: false,
};
const PDF_FILE = {
  id: 902,
  name: "doc.pdf",
  path: "/Users/test/Desktop/doc.pdf",
  category: "documents",
  extension: "pdf",
  sizeBytes: 480,
  modifiedAt: 100,
  isHidden: false,
};

function fmHandlers(overrides: HandlerMap = {}): HandlerMap {
  return {
    get_latest_scan: { value: SCAN_SUMMARY },
    get_scan_summary: { value: SCAN_SUMMARY },
    list_large_files: { value: [] },
    list_insights: { value: [] },
    list_directory_files: { value: { dirs: [], files: [], totalFiles: 0 } },
    get_directory_nodes: { value: [] },
    list_undoable_operations: { value: [] },
    list_recent_operations: { value: [] },
    get_cleanup_summary: { value: { items: [], totalBytes: 0 } },
    read_text_preview: { value: "hello from mock" },
    read_binary_preview: { value: { mimeType: "image/png", data: PNG_1PX_BASE64 } },
    ...overrides,
  };
}

async function openFileManager(page: Page, handlers: HandlerMap) {
  await page.addInitScript(() => {
    localStorage.setItem("luma.language", "en-US");
  });
  await installTauriMock(page, handlers);
  await page.goto("/");
  await page.locator("#root").waitFor();
  await page.getByRole("tab", { name: "File manager" }).click();
}

function fileRow(page: Page, name: string) {
  return page.locator(".fm-file-row").filter({ hasText: name });
}

// ── Pagination ────────────────────────────────────────────────────────────────

test("file manager: 205 files paginate across three pages", async ({ page }) => {
  const files = pagedFiles(205);
  await openFileManager(page, fmHandlers({
    list_directory_files: { value: { dirs: [], files, totalFiles: 205 } },
  }));
  const fm = page.locator(".fm-view");

  await expect(fm.getByText("Page 1 / 3")).toBeVisible();
  await expect(fileRow(page, "file-000.txt")).toBeVisible();
  await expect(fileRow(page, "file-099.txt")).toBeVisible();
  await expect(fileRow(page, "file-100.txt")).toHaveCount(0);

  await fm.getByRole("button", { name: "Next" }).click();
  await expect(fm.getByText("Page 2 / 3")).toBeVisible();
  await expect(fileRow(page, "file-100.txt")).toBeVisible();
  await expect(fileRow(page, "file-199.txt")).toBeVisible();

  await fm.getByRole("button", { name: "Previous" }).click();
  await expect(fm.getByText("Page 1 / 3")).toBeVisible();
  await expect(fileRow(page, "file-000.txt")).toBeVisible();
});

// ── Preview rendering ─────────────────────────────────────────────────────────

test("file manager: image, text and PDF previews render real content", async ({ page }) => {
  await openFileManager(page, fmHandlers({
    list_directory_files: { value: { dirs: [], files: [IMAGE_FILE, TEXT_FILE, PDF_FILE], totalFiles: 3 } },
    read_binary_preview: {
      byArgsKey: "path",
      values: {
        [IMAGE_FILE.path]: { value: { mimeType: "image/png", data: PNG_1PX_BASE64 } },
        [PDF_FILE.path]: { value: { mimeType: "application/pdf", data: minimalPdfBase64() } },
      },
    },
    read_text_preview: { value: "hello from mock" },
  }));
  const preview = page.locator("aside.fm-preview");
  page.on("console", message => {
    if (message.type() === "warning") console.log("PAGE WARN:", message.text());
  });

  await fileRow(page, "pic.png").click();
  await expect(preview.locator("img.fm-preview-image")).toBeVisible();

  await fileRow(page, "note.txt").click();
  await expect(preview.locator(".fm-preview-text")).toContainText("hello from mock");

  await fileRow(page, "doc.pdf").click();
  await expect(preview.locator("canvas")).toBeVisible({ timeout: 15_000 });
});

// ── Slow listing race guard ───────────────────────────────────────────────────

test("file manager: slow directory listing does not revert pagination", async ({ page }) => {
  const files = pagedFiles(205);
  await openFileManager(page, fmHandlers({
    list_directory_files: { value: { dirs: [], files, totalFiles: 205 }, delayMs: 400 },
  }));
  const fm = page.locator(".fm-view");

  await expect(fm.getByText("Page 1 / 3")).toBeVisible({ timeout: 10_000 });

  await fm.getByRole("button", { name: "Next" }).click();
  await expect(fm.getByText("Page 2 / 3")).toBeVisible({ timeout: 10_000 });
  await expect(fileRow(page, "file-100.txt")).toBeVisible();

  // The stale first-page response must not overwrite the newer one.
  await page.waitForTimeout(700);
  await expect(fm.getByText("Page 2 / 3")).toBeVisible();
  await expect(fileRow(page, "file-100.txt")).toBeVisible();
  await expect(fileRow(page, "file-000.txt")).toHaveCount(0);
});
