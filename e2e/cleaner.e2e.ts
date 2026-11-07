import { test, expect, type Page } from "@playwright/test";
import { installTauriMock, getTestCalls, type HandlerMap } from "./tauriMock";

// ── Fixtures ──────────────────────────────────────────────────────────────────

const OVERVIEW_FIXTURE = {
  locations: [
    {
      kind: "userCaches",
      risk: "safe",
      sizeBytes: 102_400_000,
      fileCount: 42,
      roots: ["/Users/test/Library/Caches"],
      isPartial: false,
    },
    {
      kind: "appLogs",
      risk: "safe",
      sizeBytes: 5_000_000,
      fileCount: 3,
      roots: ["/Users/test/Library/Logs"],
      isPartial: false,
    },
  ],
  volumes: [
    {
      name: "Macintosh HD",
      mountPoint: "/",
      totalBytes: 494_384_000_000,
      usedBytes: 310_000_000_000,
      freeBytes: 184_384_000_000,
    },
  ],
  generatedAt: 1_700_000_000,
};

const UNITS_FIXTURE = [
  {
    path: "/Users/test/Library/Caches/com.test.app",
    name: "com.test.app",
    sizeBytes: 90_000_000,
    modifiedAt: 1_699_000_000,
  },
  {
    path: "/Users/test/Library/Caches/com.other.app",
    name: "com.other.app",
    sizeBytes: 12_400_000,
    modifiedAt: 1_699_000_000,
  },
];

function cleanerHandlers(overrides: HandlerMap = {}): HandlerMap {
  return {
    get_latest_scan: { value: null },
    get_cleaner_overview: { value: OVERVIEW_FIXTURE },
    list_cleaner_files: { value: UNITS_FIXTURE },
    get_cleaner_whitelist: {
      value: [{ path: "/Users/test/Keep", createdAt: 1_690_000_000 }],
    },
    add_cleaner_whitelist: {
      value: [
        { path: "/Users/test/Keep", createdAt: 1_690_000_000 },
        { path: "/Users/test/New", createdAt: 1_700_000_000 },
      ],
    },
    remove_cleaner_whitelist: { value: [] },
    list_cleaner_operations: {
      value: [
        {
          id: "op-1",
          label: "磁盘管家清理（1 项，移至废纸篓）",
          status: "completed",
          createdAt: 1_699_500_000,
          finishedAt: 1_699_500_001,
        },
      ],
    },
    get_system_status: {
      value: {
        cpuUsagePercent: 12.5,
        memoryTotalBytes: 16_000_000_000,
        memoryUsedBytes: 8_000_000_000,
        battery: { percent: 87, charging: false },
        platformSupported: true,
      },
    },
    get_maintenance_tasks: { value: { tasks: [] } },
    execute_cleaner: {
      value: {
        operationId: "op-2",
        succeeded: [UNITS_FIXTURE[0].path],
        failed: [],
      },
    },
    reveal_path: { value: null },
    ...overrides,
  };
}

/** Mount the app, force English, open the cleaner tab, return its section. */
async function openCleanerTab(page: Page, handlers: HandlerMap) {
  await page.addInitScript(() => {
    localStorage.setItem("luma.language", "en-US");
  });
  await installTauriMock(page, handlers);
  await page.goto("/");
  await page.getByRole("tab", { name: "Disk cleaner" }).click();
  const section = page.locator(".cleaner-section");
  await expect(section.getByRole("heading", { name: "Disk Cleaner" })).toBeVisible();
  return section;
}

// ── Tests ────────────────────────────────────────────────────────────────────

test("disk cleaner: overview renders, location expands, units auto-selected", async ({
  page,
}) => {
  const section = await openCleanerTab(page, cleanerHandlers());

  await expect(section.getByRole("button", { name: /System & app caches/i })).toBeVisible();

  // Expand userCaches → units list appears, all selected by default.
  await section.getByRole("button", { name: /System & app caches/i }).click();
  await expect(section.getByText("com.test.app", { exact: true })).toBeVisible();
  await expect(section.getByText("com.other.app", { exact: true })).toBeVisible();

  const cleanBtn = section.getByRole("button", { name: /Clean selected/i });
  await expect(cleanBtn).toBeEnabled();
  await expect(cleanBtn).toContainText("2 items");

  const calls = await getTestCalls(page);
  const cmds = calls.map((c) => c.cmd);
  expect(cmds).toContain("get_cleaner_overview");
  expect(cmds).toContain("list_cleaner_files");
  expect(cmds).toContain("get_system_status");
});

test("disk cleaner: confirm dialog shows selection summary and cancel dismisses", async ({
  page,
}) => {
  const section = await openCleanerTab(page, cleanerHandlers());

  await section.getByRole("button", { name: /System & app caches/i }).click();
  await section.getByRole("button", { name: /Clean selected/i }).click();

  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByText("Move selected items to the Trash?")).toBeVisible();
  await expect(dialog).toContainText("2 items");

  // Deselect one unit via its `onChange` handler (the visible checkbox is
  // under the modal's event scope, so drive React state directly).
  await section
    .getByRole("checkbox", { name: "Select com.test.app" })
    .evaluate((input) => input.click());
  await expect(dialog).toContainText("1 items");

  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toBeHidden();
});

test("disk cleaner: whitelist panel lists entries and add button is enabled", async ({
  page,
}) => {
  const section = await openCleanerTab(page, cleanerHandlers());

  await section.getByRole("button", { name: /Protected paths/i }).click();
  await expect(section.getByText("/Users/test/Keep")).toBeVisible();
  await expect(section.getByRole("button", { name: "Add protected path" })).toBeEnabled();
});

test("disk cleaner: history panel loads operations", async ({ page }) => {
  const section = await openCleanerTab(page, cleanerHandlers());

  await section.getByRole("button", { name: /Cleanup history/i }).click();
  await expect(section.getByText(/磁盘管家清理/)).toBeVisible();
});

test("disk cleaner: overview failure renders alert, section still mounted", async ({
  page,
}) => {
  await page.addInitScript(() => {
    localStorage.setItem("luma.language", "en-US");
  });
  await installTauriMock(page, cleanerHandlers({
    get_cleaner_overview: { error: "WHITELIST_UNAVAILABLE" },
  }));
  await page.goto("/");
  await page.getByRole("tab", { name: "Disk cleaner" }).click();

  const section = page.locator(".cleaner-section");
  // The heading is always rendered; the alert replaces the content body.
  await expect(section.getByRole("heading", { name: "Disk Cleaner" })).toBeVisible();
  await expect(section.locator('[role="alert"]')).toBeVisible();
});

test("first open: welcome state offers a scan, no scan data needed", async ({
  page,
}) => {
  await page.addInitScript(() => {
    localStorage.setItem("luma.language", "en-US");
  });
  await installTauriMock(page, { get_latest_scan: { value: null } });
  await page.goto("/");

  await expect(page.getByRole("heading", { name: "Start with a folder" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Scan" })).toBeVisible();
});
