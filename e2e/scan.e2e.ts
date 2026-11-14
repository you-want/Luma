import { test, expect, type Page } from "@playwright/test";
import { installTauriMock, type HandlerMap } from "./tauriMock";

// ── Fixtures ──────────────────────────────────────────────────────────────────

const SCAN_SUMMARY = {
  scanId: "scan-1",
  rootPath: "/Users/test/Desktop",
  status: "completed" as const,
  startedAt: 1_700_000_000,
  totalFiles: 12_345,
  totalDirectories: 320,
  totalBytes: 10_000_000_000,
  errorCount: 0,
  categories: [
    { category: "videos", fileCount: 120, sizeBytes: 5_000_000_000 },
    { category: "images", fileCount: 4_500, sizeBytes: 3_000_000_000 },
    { category: "documents", fileCount: 4_800, sizeBytes: 1_500_000_000 },
    { category: "other", fileCount: 2_925, sizeBytes: 500_000_000 },
  ],
};

const LARGE_FILES_FIXTURE = [
  {
    id: 1,
    name: "movie.mp4",
    path: "/Users/test/Desktop/videos/movie.mp4",
    category: "videos",
    sizeBytes: 2_000_000_000,
    modifiedAt: 1_690_000_000,
    isHidden: false,
  },
  {
    id: 2,
    name: "archive.iso",
    path: "/Users/test/Desktop/archive.iso",
    category: "images",
    sizeBytes: 1_500_000_000,
    modifiedAt: 1_680_000_000,
    isHidden: false,
  },
];

function scanHandlers(overrides: HandlerMap = {}): HandlerMap {
  return {
    get_latest_scan: { value: SCAN_SUMMARY },
    get_scan_summary: { value: SCAN_SUMMARY },
    list_large_files: { value: LARGE_FILES_FIXTURE },
    list_insights: { value: [] },
    list_directory_files: { value: { dirs: [], files: [], totalFiles: 12 } },
    get_cleanup_summary: { value: { items: [], totalBytes: 0 } },
    list_recent_operations: { value: [] },
    get_cleaner_overview: { value: { locations: [], volumes: [], generatedAt: 1_700_000_000 } },
    get_system_status: {
      value: { cpuUsagePercent: 0, memoryTotalBytes: 0, memoryUsedBytes: 0, battery: null, platformSupported: true },
    },
    get_maintenance_tasks: { value: { tasks: [] } },
    get_cleaner_whitelist: { value: [] },
    list_cleaner_operations: { value: [] },
    ...overrides,
  };
}

/**
 * Install the mock as an init script, then navigate once. The app's
 * mount-time `getLatestScan()` resolves against the fixture, driving the
 * ready / loaded path without any reload trick.
 */
async function openWithScan(page: Page, handlers: HandlerMap) {
  await page.addInitScript(() => {
    localStorage.setItem("luma.language", "en-US");
  });
  await installTauriMock(page, handlers);
  await page.goto("/");
  await page.locator("#root").waitFor();
}
// ── Tests ────────────────────────────────────────────────────────────────────

test("with a previous scan: analysis tab shows category breakdown and large files", async ({
  page,
}) => {
  await openWithScan(page, scanHandlers());

  // The app now lands on the Overview dashboard; the analysis view is one
  // sidebar click away once the restored scan has loaded.
  const panel = page.locator("#workspace-panel-analysis");
  await expect(page.getByRole("tab", { name: "Space analysis" })).toBeEnabled({ timeout: 15_000 });
  await page.getByRole("tab", { name: "Space analysis" }).click();
  await expect(panel).toBeVisible();

  // Category list rendered.
  await expect(panel.locator(".legend-label", { hasText: "Videos" })).toBeVisible();
  await expect(panel.locator(".legend-label", { hasText: "Images" })).toBeVisible();

  // Large files panel populated from the fixture.
  await expect(panel.locator(".file-list strong", { hasText: "movie.mp4" }).first()).toBeVisible();
  await expect(panel.locator(".file-list strong", { hasText: "archive.iso" }).first()).toBeVisible();
});

test("no previous scan: overview welcome state with scan CTA, workspace tabs disabled", async ({
  page,
}) => {
  await page.addInitScript(() => {
    localStorage.setItem("luma.language", "en-US");
  });
  await installTauriMock(page, { get_latest_scan: { value: null } });
  await page.goto("/");
  await page.locator("#root").waitFor();
  // The Overview dashboard is the landing view and hosts the welcome state.
  await expect(page.getByRole("heading", { name: "Start with a folder" })).toBeVisible({ timeout: 15_000 });

  // No result yet: workspace tabs are disabled; overview + machine tabs work.
  await expect(page.getByRole("tab", { name: "Overview" })).toBeEnabled();
  await expect(page.getByRole("tab", { name: "Space analysis" })).toBeDisabled();
  await expect(page.getByRole("tab", { name: "File manager" })).toBeDisabled();
  await expect(page.getByRole("tab", { name: "Organize" })).toBeDisabled();
  await expect(page.getByRole("tab", { name: "Disk cleaner" })).toBeEnabled();
});

test("overview dashboard: organize card counts loose files and routes to the organize page", async ({
  page,
}) => {
  await openWithScan(page, scanHandlers());

  const card = page.locator(".overview-card", { hasText: "To organize" });
  await expect(card).toBeVisible({ timeout: 15_000 });
  await expect(card.getByText("12 files")).toBeVisible();

  await card.getByRole("button", { name: "Start organizing" }).click();
  await expect(page.getByRole("tab", { name: "Organize" })).toHaveAttribute("aria-selected", "true");
  await expect(page.locator("#workspace-panel-organize")).toBeVisible();
  await expect(page.getByRole("heading", { name: "Choose organizing rule" })).toBeVisible();
});

test("scan summary error surfaces as an alert on the analysis tab", async ({
  page,
}) => {
  await openWithScan(page, scanHandlers({
    get_scan_summary: { error: "SCAN_NOT_FOUND" },
  }));
  // The error path replaces the analysis panel with a top-level status alert.
  await expect(page.locator('[role="alert"]').first()).toBeVisible({ timeout: 15_000 });
  await expect(page.getByText("Scan ran into a problem")).toBeVisible();
});
