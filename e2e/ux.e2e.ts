import { test, expect, type Page } from "@playwright/test";
import { installTauriMock, getTestCalls, type HandlerMap } from "./tauriMock";

// ── Fixtures ──────────────────────────────────────────────────────────────────

const SCAN_SUMMARY = {
  scanId: "scan-1",
  rootPath: "/Users/test/Desktop",
  status: "completed" as const,
  startedAt: 1_700_000_000,
  finishedAt: 1_700_000_050,
  totalFiles: 2,
  totalDirectories: 1,
  totalBytes: 4_000_000_000,
  errorCount: 0,
  categories: [
    { category: "videos", fileCount: 1, sizeBytes: 2_000_000_000 },
    { category: "documents", fileCount: 1, sizeBytes: 2_000_000_000 },
  ],
};

const FILE_A = {
  id: 1,
  name: "a.txt",
  path: "/Users/test/Desktop/a.txt",
  category: "documents",
  sizeBytes: 100,
  modifiedAt: 100,
  isHidden: false,
};
const FILE_B = {
  id: 2,
  name: "b.txt",
  path: "/Users/test/Desktop/b.txt",
  category: "documents",
  sizeBytes: 100,
  modifiedAt: 200,
  isHidden: false,
};
const MOVIE = {
  id: 3,
  name: "movie.mp4",
  path: "/Users/test/Desktop/movie.mp4",
  category: "videos",
  sizeBytes: 2_000_000_000,
  modifiedAt: 1_690_000_000,
  isHidden: false,
};

const OPERATIONS_LOG = [
  {
    id: "op-1",
    scanId: "scan-1",
    rootPath: "/Users/test/Desktop",
    kind: "organize",
    label: "Organize Desktop · 12 files",
    status: "completed",
    createdAt: 1_700_000_000,
    finishedAt: 1_700_000_100,
    canUndo: true,
  },
  {
    id: "op-2",
    scanId: "cleaner",
    rootPath: "",
    kind: "trash",
    label: "Package manager caches 1.9 GB",
    status: "completed",
    createdAt: 1_699_900_000,
    finishedAt: 1_699_900_050,
    canUndo: false,
  },
  {
    id: "op-3",
    scanId: "scan-old",
    rootPath: "/Users/test/Downloads",
    kind: "rename",
    label: "old.png renamed",
    status: "completed",
    createdAt: 1_699_800_000,
    finishedAt: 1_699_800_010,
    canUndo: true,
  },
];

function uxHandlers(overrides: HandlerMap = {}): HandlerMap {
  return {
    get_latest_scan: { value: SCAN_SUMMARY },
    get_scan_summary: { value: SCAN_SUMMARY },
    list_large_files: { value: [MOVIE] },
    list_insights: { value: [] },
    list_directory_files: { value: { dirs: [], files: [FILE_A, FILE_B], totalFiles: 2 } },
    get_directory_nodes: { value: [] },
    list_undoable_operations: { value: [] },
    list_recent_operations: { value: OPERATIONS_LOG },
    get_cleanup_summary: { value: { items: [], totalBytes: 0 } },
    list_cleanup_files: { value: [] },
    find_duplicates: {
      value: [{
        contentHash: "hash-1",
        sizeBytes: 100,
        fileCount: 2,
        wastedBytes: 100,
        files: [FILE_A, FILE_B],
      }],
    },
    trash_files: { value: { succeeded: [FILE_A.path], failed: [] } },
    plan_organize: {
      value: {
        ruleName: "By file type",
        sourceDir: "/Users/test/Desktop",
        destDir: "/Users/test/Desktop",
        moves: [{
          from: FILE_A.path,
          to: "/Users/test/Desktop/Images/a.txt",
          fromName: FILE_A.name,
          subfolder: "Images",
          groupKey: "cat-images",
          conflict: false,
          renamed: false,
          expectedSizeBytes: 100,
          expectedModifiedAt: 100,
        }],
        conflictCount: 0,
        alreadyPlaced: 0,
        preservedFiles: 0,
      },
    },
    execute_organize_plan: { value: { operationId: "op-org", succeeded: 1, failed: [] } },
    undo_file_operation: { value: { operationId: "op-1", succeeded: [FILE_A.path], failed: [] } },
    get_tray_monitor_enabled: { value: true },
    set_tray_monitor_enabled: { value: null },
    get_cleaner_whitelist: { value: [{ path: "/Users/test/Documents/contracts", createdAt: 1_700_000_000 }] },
    get_cleaner_overview: { value: { locations: [], volumes: [], generatedAt: 1_700_000_000 } },
    get_system_status: {
      value: { cpuUsagePercent: 0, memoryTotalBytes: 0, memoryUsedBytes: 0, battery: null, platformSupported: true },
    },
    get_maintenance_tasks: { value: { tasks: [] } },
    list_cleaner_operations: { value: [] },
    ...overrides,
  };
}

async function openApp(page: Page, handlers: HandlerMap) {
  await page.addInitScript(() => {
    localStorage.setItem("luma.language", "en-US");
  });
  await installTauriMock(page, handlers);
  await page.goto("/");
  await page.locator("#root").waitFor();
}

async function openTab(page: Page, name: string | RegExp) {
  await page.getByRole("tab", { name }).click();
}

// ── File manager smoke ────────────────────────────────────────────────────────

test("file manager: lists files, batch selection enables trash with confirmation", async ({ page }) => {
  await openApp(page, uxHandlers());
  await openTab(page, "File manager");
  const fm = page.locator(".fm-view");
  await expect(fm.getByText("a.txt", { exact: true })).toBeVisible();

  await fm.getByRole("checkbox", { name: "Select a.txt" }).click();
  await expect(fm.getByText("1 selected")).toBeVisible();

  await fm.getByRole("button", { name: "Trash", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "Move to trash" }).click();

  const calls = await getTestCalls(page);
  const trash = calls.find((c) => c.cmd === "trash_files");
  expect(trash).toBeDefined();
  expect(trash!.args.scanId).toBe("scan-1");
  expect(trash!.args.paths).toEqual([FILE_A.path]);
});

// ── Organize full flow ────────────────────────────────────────────────────────

test("organize page: rule → preview → execute completes and offers undo", async ({ page }) => {
  await openApp(page, uxHandlers());
  await openTab(page, "Organize");
  const org = page.locator(".org-page");
  await expect(org).toBeVisible();

  await org.getByRole("button", { name: "Next" }).click();
  await expect(org.getByText("/Users/test/Desktop").first()).toBeVisible();
  await org.getByRole("button", { name: "Generate preview" }).click();

  await expect(org.getByText("Images").first()).toBeVisible();
  await org.getByRole("button", { name: /Confirm moving 1 files/ }).click();

  await expect(org.getByRole("heading", { name: "Organize complete" })).toBeVisible();
  await expect(org.getByRole("button", { name: "Undo this organize run" })).toBeEnabled();

  const calls = await getTestCalls(page);
  const executed = calls.find((c) => c.cmd === "execute_organize_plan");
  expect(executed).toBeDefined();
  expect(executed!.args.scanId).toBe("scan-1");
});

// ── History ───────────────────────────────────────────────────────────────────

test("history: journal rows, undo uses the active scan id, cleaner rows point to trash", async ({ page }) => {
  await openApp(page, uxHandlers());
  await openTab(page, "History");
  const list = page.locator(".history-list");
  await expect(list.locator(".history-row")).toHaveCount(3);

  // The organize entry of the current root is undoable here.
  const row1 = list.locator(".history-row", { hasText: "Organize Desktop" });
  await row1.getByRole("button", { name: "Undo" }).click();
  await expect(page.locator(".fm-toast").first()).toBeVisible();

  const calls = await getTestCalls(page);
  const undo = calls.find((c) => c.cmd === "undo_file_operation");
  expect(undo).toBeDefined();
  expect(undo!.args.operationId).toBe("op-1");
  // The undo command must receive the CURRENT scan id, not the journal row's.
  expect(undo!.args.scanId).toBe("scan-1");

  // Cleaner entries are recoverable from the trash, not undoable in-app.
  await expect(list.locator(".history-row", { hasText: "Package manager caches" }))
    .toContainText("Recoverable from trash");
  // Undoable entries from another root ask for that folder to be scanned.
  await expect(list.locator(".history-row", { hasText: "old.png" }))
    .toContainText("Scan the folder to undo");

  // Type filter narrows the journal.
  await page.getByRole("button", { name: "Cleanup & sessions" }).click();
  await expect(list.locator(".history-row")).toHaveCount(1);
});

// ── Settings ──────────────────────────────────────────────────────────────────

test("settings: renders sections, whitelist paths, tray switch calls the backend", async ({ page }) => {
  await openApp(page, uxHandlers());
  await openTab(page, "Settings");

  await expect(page.getByRole("heading", { name: "General" })).toBeVisible();
  await expect(page.getByText("/Users/test/Documents/contracts")).toBeVisible();
  await expect(page.getByText("Luma · v0.4.1")).toBeVisible();

  // Tray monitor starts on (mocked); toggling calls set_tray_monitor_enabled(false).
  const trayCard = page.locator(".settings-card", { hasText: "Tray" });
  await trayCard.locator(".switch").click();
  const calls = await getTestCalls(page);
  const set = calls.find((c) => c.cmd === "set_tray_monitor_enabled");
  expect(set).toBeDefined();
  expect(set!.args.enabled).toBe(false);
});

// ── Duplicates: keep newest, trash the rest ───────────────────────────────────

test("duplicates: keep-newest action trashes only the older copy", async ({ page }) => {
  await openApp(page, uxHandlers());
  await openTab(page, "Space analysis");
  const section = page.locator(".duplicates-section");
  await section.getByRole("button", { name: "Find duplicates" }).click();

  const group = section.locator(".duplicate-group");
  await expect(group.getByText("2 identical files")).toBeVisible();
  await group.getByRole("button", { name: /2 identical files/ }).click();

  await expect(group.getByText(/Keep the newest copy/)).toBeVisible();
  await group.getByRole("button", { name: "Keep newest, trash the rest (1)" }).click();

  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByText("Keeping the newest copy: b.txt")).toBeVisible();
  await dialog.getByRole("button", { name: "Move to trash" }).click();

  const calls = await getTestCalls(page);
  const trash = calls.find((c) => c.cmd === "trash_files");
  expect(trash).toBeDefined();
  // The newer copy (b.txt) stays; only the older a.txt goes to the trash.
  expect(trash!.args.paths).toEqual([FILE_A.path]);
});

// ── Cleanup hints routing ─────────────────────────────────────────────────────

test("cleanup hints: dev artifacts route into the organize page", async ({ page }) => {
  await openApp(page, uxHandlers({
    get_cleanup_summary: {
      value: { items: [{ kind: "development", sizeBytes: 2_048, fileCount: 4 }], totalBytes: 2_048 },
    },
  }));
  await openAppTabAnalysis(page);
  const section = page.locator(".cleanup-section");
  await section.getByRole("button", { name: /Dev Build Artifacts/ }).click();
  await section.getByRole("button", { name: "Organize", exact: true }).click();

  await expect(page.getByRole("tab", { name: "Organize" })).toHaveAttribute("aria-selected", "true");
  await expect(page.locator("#workspace-panel-organize")).toBeVisible();
});

// ── Insight rows: per-file trash ──────────────────────────────────────────────

test("largest files: row trash action confirms and calls trash_files", async ({ page }) => {
  await openApp(page, uxHandlers());
  await openAppTabAnalysis(page);
  const row = page.locator(".large-files-section, .result-section", { hasText: "movie.mp4" })
    .locator(".file-row", { hasText: "movie.mp4" }).first();
  await row.getByRole("button", { name: "Move movie.mp4 to trash" }).click();

  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "Move to trash" }).click();

  const calls = await getTestCalls(page);
  const trash = calls.find((c) => c.cmd === "trash_files");
  expect(trash).toBeDefined();
  expect(trash!.args.paths).toEqual([MOVIE.path]);
});

async function openAppTabAnalysis(page: Page) {
  await openTab(page, "Space analysis");
}
