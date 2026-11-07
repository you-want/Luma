import { test, expect, type Page } from "@playwright/test";
import { installTauriMock, type HandlerMap } from "./tauriMock";

// ── Fixtures ──────────────────────────────────────────────────────────────────

const NOW = Date.now() / 1000;

const SESSIONS_REPORT = {
  agents: [
    { id: "claude", sessionCount: 2 },
    { id: "codex", sessionCount: 1 },
  ],
  sessions: [
    {
      id: "s-1",
      agent: "claude",
      projectDir: "/Users/t/repo-a",
      projectName: "repo-a",
      title: "Fix login bug",
      summary: "Fixed and covered by tests.",
      firstMessage: "Fix login bug",
      createdAt: NOW - 3600,
      lastActiveAt: NOW - 60,
      sizeBytes: 1234,
      sourcePath: "/Users/t/.claude/projects/repo-a/s-1.jsonl",
      resumeCommand: "claude --resume s-1",
    },
    {
      id: "s-2",
      agent: "claude",
      projectDir: "/Users/t/repo-a",
      projectName: "repo-a",
      title: "Refactor parser",
      summary: "Parser split into modules.",
      firstMessage: "Refactor parser",
      createdAt: NOW - 40 * 86_400,
      lastActiveAt: NOW - 40 * 86_400,
      sizeBytes: 2048,
      sourcePath: "/Users/t/.claude/projects/repo-a/s-2.jsonl",
      resumeCommand: "claude --resume s-2",
    },
    {
      id: "s-3",
      agent: "codex",
      projectDir: "/Users/t/repo-b",
      projectName: "repo-b",
      title: "收口快捷键",
      summary: "阻止浏览器默认行为。",
      firstMessage: "收口快捷键",
      createdAt: NOW - 7200,
      lastActiveAt: NOW - 7200,
      sizeBytes: 456,
      sourcePath: "/Users/t/.codex/sessions/2026/11/06/rollout-s-3.jsonl",
      resumeCommand: "codex resume s-3",
    },
  ],
};

function sessionsHandlers(overrides: HandlerMap = {}): HandlerMap {
  return {
    get_latest_scan: { value: null },
    discover_agent_sessions: { value: SESSIONS_REPORT },
    trash_agent_sessions: {
      value: { operationId: "op-sessions", succeeded: [SESSIONS_REPORT.sessions[0].sourcePath], failed: [] },
    },
    ...overrides,
  };
}

async function openSessionsTab(page: Page, handlers: HandlerMap) {
  await page.addInitScript(() => {
    localStorage.setItem("luma.language", "en-US");
  });
  await installTauriMock(page, handlers);
  await page.goto("/");
  await page.getByRole("tab", { name: "Sessions", exact: true }).click();
  const panel = page.locator(".sessions-panel");
  await expect(panel.getByRole("heading", { name: "Sessions", exact: true })).toBeVisible();
  return panel;
}

// ── Grouping & filtering ──────────────────────────────────────────────────────

test("sessions tab: lists sessions grouped by project with counts", async ({ page }) => {
  const panel = await openSessionsTab(page, sessionsHandlers());

  const repoA = panel.locator(".sessions-group", { hasText: "repo-a" });
  await expect(repoA.getByText("2 sessions")).toBeVisible();
  await expect(repoA.getByText("Fix login bug")).toBeVisible();
  await expect(panel.locator(".sessions-group", { hasText: "repo-b" }).getByText("收口快捷键"))
    .toBeVisible();

  // Agent dropdown carries per-agent counts.
  await expect(panel.locator(".sessions-agent-select")).toContainText("All agents · 3");
  await expect(panel.locator(".sessions-agent-select")).toContainText("Claude Code · 2");
});

test("sessions tab: agent filter narrows the list", async ({ page }) => {
  const panel = await openSessionsTab(page, sessionsHandlers());

  await panel.locator(".sessions-agent-select").selectOption({ label: "Codex · 1" });
  await expect(panel.locator(".sessions-group", { hasText: "repo-b" })).toBeVisible();
  await expect(panel.locator(".sessions-group", { hasText: "repo-a" })).toHaveCount(0);
});

test("sessions tab: search matches title, project and message text", async ({ page }) => {
  const panel = await openSessionsTab(page, sessionsHandlers());

  await panel.getByPlaceholder("Search title, folder, first/last message or session ID")
    .fill("Fixed and covered");
  await expect(panel.locator(".session-row")).toHaveCount(1);
  await expect(panel.locator(".session-row").getByText("Fix login bug")).toBeVisible();
});

test("sessions tab: grouping by time shows buckets", async ({ page }) => {
  const panel = await openSessionsTab(page, sessionsHandlers());

  await panel.getByRole("button", { name: "By time" }).click();
  await expect(panel.locator(".sessions-group", { hasText: "Today" })).toBeVisible();
  // The 40-day-old session falls into the last bucket.
  await expect(panel.locator(".sessions-group", { hasText: "Earlier" })).toBeVisible();
});

// ── Deletion ──────────────────────────────────────────────────────────────────

test("sessions tab: group select-all selects one project's sessions and deletes them", async ({ page }) => {
  const repoAPaths = SESSIONS_REPORT.sessions
    .filter((session) => session.projectName === "repo-a")
    .map((session) => session.sourcePath);
  const panel = await openSessionsTab(page, sessionsHandlers({
    trash_agent_sessions: {
      value: { operationId: "op-group", succeeded: repoAPaths, failed: [] },
    },
  }));

  // Check the group checkbox: every session under repo-a becomes selected…
  const repoA = panel.locator(".sessions-group", { hasText: "repo-a" });
  await repoA.getByRole("checkbox", { name: /Select all sessions in "repo-a"/ }).check();
  await expect(panel.getByRole("checkbox", { name: "Fix login bug" })).toBeChecked();
  await expect(panel.getByRole("checkbox", { name: "Refactor parser" })).toBeChecked();
  // …and repo-b is untouched.
  await expect(panel.getByRole("checkbox", { name: "收口快捷键" })).not.toBeChecked();
  await expect(panel.getByText("2 / 3 selected")).toBeVisible();

  // Unchecking clears exactly that group again.
  await repoA.getByRole("checkbox", { name: /Select all sessions in "repo-a"/ }).uncheck();
  await expect(panel.getByRole("checkbox", { name: "Fix login bug" })).not.toBeChecked();

  // Select again and delete the whole project's sessions.
  await repoA.getByRole("checkbox", { name: /Select all sessions in "repo-a"/ }).check();
  await panel.getByRole("button", { name: "Delete selected" }).click();
  await page.locator(".fm-dialog").getByRole("button", { name: "Move to Trash" }).click();
  await expect(panel.locator(".sessions-result")).toContainText("2 sessions moved to the Trash");
});

test("sessions tab: select-all and batch delete move sessions to the trash", async ({ page }) => {
  const panel = await openSessionsTab(page, sessionsHandlers());

  // One repo-a session + the repo-b session, then select all filtered rows.
  await panel.locator(".session-row").first().getByRole("checkbox").check();
  await panel.getByRole("checkbox", { name: "Select all", exact: true }).check();

  await panel.getByRole("button", { name: "Delete selected" }).click();
  await expect(page.locator(".fm-dialog")).toBeVisible();
  await page.locator(".fm-dialog").getByRole("button", { name: "Move to Trash" }).click();

  await expect(panel.locator(".sessions-result")).toContainText("1 sessions moved to the Trash");
});

test("sessions tab: no sessions shows the empty state", async ({ page }) => {
  const panel = await openSessionsTab(page, sessionsHandlers({
    discover_agent_sessions: {
      value: { agents: [{ id: "claude", sessionCount: 0 }], sessions: [] },
    },
  }));
  await expect(panel.getByText("No local sessions found")).toBeVisible();
});

test("sessions tab: discovery failure renders alert", async ({ page }) => {
  const panel = await openSessionsTab(page, sessionsHandlers({
    discover_agent_sessions: { error: "SESSIONS_UNAVAILABLE" },
  }));
  await expect(panel.locator('[role="alert"]')).toBeVisible();
});
