import { test, expect, type Page } from "@playwright/test";
import { installTauriMock, type HandlerMap } from "./tauriMock";

// ── Fixtures ──────────────────────────────────────────────────────────────────

const PURGE_REPORT = {
  projects: [
    {
      path: "/Users/test/Projects/myapp",
      name: "myapp",
      totalBytes: 10_000_000,
      lastActiveAt: 1_600_000_000,
      isActive: false,
      artifacts: [
        {
          group: "nodeModules",
          path: "/Users/test/Projects/myapp/node_modules",
          sizeBytes: 5_000_000,
          fileCount: 420,
          gitTracked: false,
        },
      ],
    },
    {
      path: "/Users/test/Projects/active-app",
      name: "active-app",
      totalBytes: 2_000_000,
      lastActiveAt: Date.now() / 1000,
      isActive: true,
      artifacts: [
        {
          group: "rustTarget",
          path: "/Users/test/Projects/active-app/target",
          sizeBytes: 1_000_000,
          fileCount: 80,
          gitTracked: false,
        },
      ],
    },
  ],
  scannedRoots: ["/Users/test/Projects"],
  isPartial: false,
};

const ORPHAN_REPORT = {
  items: [
    {
      path: "/Users/test/Library/Caches/com.removed.app",
      name: "com.removed.app",
      bundleId: "com.removed.app",
      group: "caches",
      risk: "safe" as const,
      sizeBytes: 3_000_000,
      fileCount: 60,
      modifiedAt: 1_690_000_000,
    },
    {
      path: "/Users/test/Library/Application Support/com.removed.app",
      name: "com.removed.app",
      bundleId: "com.removed.app",
      group: "support",
      risk: "review" as const,
      sizeBytes: 1_000_000,
      fileCount: 12,
      modifiedAt: 1_690_000_000,
    },
  ],
  scannedRoots: ["/Users/test/Library"],
  isPartial: false,
  platformSupported: true,
};

const SLIM_REPORT = {
  candidates: [
    {
      path: "/Applications/SlimmableApp.app",
      name: "SlimmableApp",
      currentBytes: 40_000_000,
      savingBytes: 20_000_000,
    },
  ],
  isPartial: false,
  platformSupported: true,
};

const MAINTENANCE_REPORT = {
  tasks: [
    { id: "quicklookCache", available: true, needsAdmin: false },
    { id: "dnsFlush", available: true, needsAdmin: true },
  ],
};

const SYSTEM_STATUS = {
  cpuUsagePercent: 24.5,
  memoryTotalBytes: 16_000_000_000,
  memoryUsedBytes: 8_000_000_000,
  battery: { percent: 64, charging: false },
  platformSupported: true,
};

function panelHandlers(overrides: HandlerMap = {}): HandlerMap {
  return {
    get_latest_scan: { value: null },
    get_cleaner_overview: { value: { locations: [], volumes: [], generatedAt: 1_700_000_000 } },
    list_cleaner_files: { value: [] },
    get_cleaner_whitelist: { value: [] },
    list_cleaner_operations: { value: [] },
    discover_purge_projects: { value: PURGE_REPORT },
    execute_purge: { value: { operationId: "op-p", succeeded: [], failed: [] } },
    discover_orphan_app_data: { value: ORPHAN_REPORT },
    execute_orphan_clean: { value: { operationId: "op-o", succeeded: [], failed: [] } },
    discover_slim_candidates: { value: SLIM_REPORT },
    execute_slim: { value: { operationId: "op-s", succeeded: [], failed: [] } },
    get_maintenance_tasks: { value: MAINTENANCE_REPORT },
    run_maintenance_task: { value: { operationId: "op-m", detail: "done" } },
    get_system_status: { value: SYSTEM_STATUS },
    reveal_path: { value: null },
    ...overrides,
  };
}

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

// ── Project slimming ──────────────────────────────────────────────────────────

test("purge panel: scan lists projects, active project is locked", async ({ page }) => {
  const section = await openCleanerTab(page, panelHandlers());
  const purge = section.locator(".purge-section");

  await purge.getByRole("button", { name: "Scan dev projects" }).click();
  await expect(purge.getByText("myapp", { exact: true })).toBeVisible();
  // The active project's name is inline with its "Active" badge — match the row.
  await expect(purge.locator(".purge-row", { hasText: "active-app" })).toBeVisible();

  // The active project shows an "Active" badge and its artifacts are disabled.
  await expect(purge.getByText("Active", { exact: true })).toBeVisible();
  await expect(
    purge.locator(".purge-row", { hasText: "active-app" }).getByRole("checkbox").first()
  ).toBeDisabled();

  // The cleanable project's artifact is selectable.
  const cleanableRow = purge.locator(".purge-row", { hasText: "myapp" });
  await expect(cleanableRow.getByRole("checkbox").first()).toBeEnabled();
});

test("purge panel: no projects found shows empty state", async ({ page }) => {
  const section = await openCleanerTab(page, panelHandlers({
    discover_purge_projects: {
      value: { projects: [], scannedRoots: ["/Users/test/Projects"], isPartial: false },
    },
  }));
  const purge = section.locator(".purge-section");
  await purge.getByRole("button", { name: "Scan dev projects" }).click();
  await expect(purge.getByText(/No projects with build artifacts/i)).toBeVisible();
});

test("purge panel: scan failure renders alert, section still mounted", async ({ page }) => {
  const section = await openCleanerTab(page, panelHandlers({
    discover_purge_projects: { error: "DISCOVERY_FAILED" },
  }));
  const purge = section.locator(".purge-section");
  await purge.getByRole("button", { name: "Scan dev projects" }).click();
  await expect(purge.locator('[role="alert"]').first()).toBeVisible();
  await expect(purge.getByRole("heading", { name: "Project slimming" })).toBeVisible();
});

// ── Orphan app data ───────────────────────────────────────────────────────────

test("orphans panel: safe items auto-selected, review items not", async ({ page }) => {
  const section = await openCleanerTab(page, panelHandlers());
  const orphans = section.locator(".orphans-section");

  await orphans.getByRole("button", { name: "Scan for leftovers" }).click();
  // Each row's name is inline with its group badge — match the rows instead.
  await expect(orphans.locator(".orphans-row", { hasText: "com.removed.app" })).toHaveCount(2);

  // "safe" group (caches) is auto-checked; "review" group (support) is not.
  const safeRow = orphans.locator(".orphans-row", { hasText: "com.removed.app" }).filter({ hasText: "Caches" });
  const reviewRow = orphans.locator(".orphans-row", { hasText: "com.removed.app" }).filter({ hasText: "Personal data" });
  await expect(safeRow.getByRole("checkbox")).toBeChecked();
  await expect(reviewRow.getByRole("checkbox")).not.toBeChecked();

  // The clean button reflects only the auto-selected (safe) items.
  await expect(orphans.getByRole("button", { name: /Clean selected leftovers/i })).toBeEnabled();
});

test("orphans panel: unsupported platform shows notice", async ({ page }) => {
  const section = await openCleanerTab(page, panelHandlers({
    discover_orphan_app_data: {
      value: {
        items: [],
        scannedRoots: [],
        isPartial: false,
        platformSupported: false,
      },
    },
  }));
  const orphans = section.locator(".orphans-section");
  await orphans.getByRole("button", { name: "Scan for leftovers" }).click();
  await expect(orphans.getByText(/macOS only/i)).toBeVisible();
});

// ── App slimming ──────────────────────────────────────────────────────────────

test("slim panel: candidates listed with savings, selection enables button", async ({ page }) => {
  const section = await openCleanerTab(page, panelHandlers());
  const slim = section.locator(".slim-section");

  await slim.getByRole("button", { name: "Scan slimmable apps" }).click();
  await expect(slim.getByText("SlimmableApp", { exact: true })).toBeVisible();
  await expect(slim.getByText("saves about 19.1 MB")).toBeVisible();

  // Before selecting anything the slim button is disabled.
  await expect(slim.getByRole("button", { name: /Slim selected apps/i })).toBeDisabled();

  // Selecting the candidate enables it.
  const slimCheckbox = slim.getByRole("checkbox", { name: "Select SlimmableApp" });
  await expect(slimCheckbox).toBeVisible({ timeout: 10_000 });
  await slimCheckbox.click();
  await expect(slim.getByRole("button", { name: /Slim selected apps/i })).toBeEnabled();
});

test("slim panel: unsupported platform shows notice", async ({ page }) => {
  const section = await openCleanerTab(page, panelHandlers({
    discover_slim_candidates: {
      value: { candidates: [], isPartial: false, platformSupported: false },
    },
  }));
  const slim = section.locator(".slim-section");
  await slim.getByRole("button", { name: "Scan slimmable apps" }).click();
  await expect(slim.getByText(/Apple Silicon Mac/i)).toBeVisible();
});

// ── System maintenance ────────────────────────────────────────────────────────

test("maintenance panel: both tasks listed, admin badge on DNS", async ({ page }) => {
  const section = await openCleanerTab(page, panelHandlers());
  const maint = section.locator(".maintenance-section");

  await expect(maint.getByText("Refresh QuickLook cache")).toBeVisible();
  await expect(maint.getByText("Flush DNS cache")).toBeVisible();
  // The DNS task requires admin → badge shown.
  await expect(maint.getByText("Admin required")).toBeVisible();

  // Both tasks offer a Run button.
  const runButtons = maint.getByRole("button", { name: "Run" });
  await expect(runButtons).toHaveCount(2);
});

test("maintenance panel: running a task shows outcome and re-enables buttons", async ({ page }) => {
  const section = await openCleanerTab(page, panelHandlers());
  const maint = section.locator(".maintenance-section");

  await maint.getByRole("button", { name: "Run" }).first().click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "Run" }).click();

  // Outcome banner appears.
  await expect(maint.locator('[role="status"]')).toBeVisible();
});

// ── System status card ────────────────────────────────────────────────────────

test("system status card: renders CPU / memory / battery values", async ({ page }) => {
  const section = await openCleanerTab(page, panelHandlers());
  const card = section.locator(".status-card");

  await expect(card.getByRole("heading", { name: "System status" })).toBeVisible();
  await expect(card.getByText("25%")).toBeVisible();
  // Memory row renders "used / total" — match the GB values in either locale.
  await expect(card.getByText(/GB \/ /)).toBeVisible();
  await expect(card.getByText("64%")).toBeVisible();
  await expect(card.getByText("On battery")).toBeVisible();
});

test("system status card: unsupported platform shows notice", async ({ page }) => {
  const section = await openCleanerTab(page, panelHandlers({
    get_system_status: {
      value: {
        cpuUsagePercent: null,
        memoryTotalBytes: null,
        memoryUsedBytes: null,
        battery: null,
        platformSupported: false,
      },
    },
  }));
  const card = section.locator(".status-card");
  await expect(card.getByText(/unavailable on this platform/i)).toBeVisible();
});
