// Tauri IPC mock for Playwright e2e tests.
//
// The Vite dev server has no Tauri shell, so `window.__TAURI_INTERNALS__`
// is absent and every `invoke` from `src/lib/tauri.ts` throws. This helper
// installs a deterministic `__TAURI_INTERNALS__` so tests drive happy-path
// UI flows with fixture data.
//
// ## How the install works
//
// `installTauriMock` registers an init script (`page.addInitScript`) that
// runs in the page's main world *before* any module script executes. That
// means `main.tsx`'s mount-time `getLatestScan()` already sees the bridge,
// so a single `page.goto` drives the ready / loaded path — no reload trick
// needed.
//
// Handler maps are plain JSON (no functions) because they cross the
// init-script serialization boundary. Each entry is a *return spec*:
//
//   { value?: unknown, error?: string }
//
//   - `value` — resolved by the mocked `invoke`
//   - `error` — if set, the call rejects with `new Error(error)`
//
// Unknown commands reject with `new Error(`[mock] no handler …`)`, so the
// app's error paths surface as testable alerts instead of silent hangs.
//
// Call `installTauriMock` **before** `page.goto` in each test.

import type { Page } from "@playwright/test";

export type HandlerSpec =
  | undefined
  | {
      value?: unknown;
      error?: string;
    };

export type HandlerMap = Record<string, HandlerSpec>;

export interface TestCall {
  cmd: string;
  args: Record<string, unknown>;
  callNo: number;
}

/**
 * Install the Tauri IPC mock as an init script. Must run **before**
 * `page.goto` so the app's mount-time `invoke` calls resolve against the
 * fixtures.
 */
export async function installTauriMock(page: Page, handlers: HandlerMap) {
  const map: Record<string, Record<string, unknown>> = {};
  for (const [cmd, spec] of Object.entries(handlers)) {
    if (spec) map[cmd] = { value: spec.value ?? null, error: spec.error };
  }

  // Plugin commands the app calls at module scope that have no fixture:
  // event listeners (scan-progress / scan-finished / organize-progress),
  // version, updater check. Without these, `useScanEvents`'s `listen`
  // rejects and the app's scan state machine throws unhandled rejections.
  const pluginDefaults: Record<string, Record<string, unknown>> = {
    "plugin:event|listen": { value: "mock-event-id" },
    "plugin:event|unlisten": { value: null },
    "plugin:app|version": { value: "0.4.1" },
    "plugin:updater|check": { value: { available: false } },
    "plugin:process|exit": { value: null },
    "plugin:process|relaunch": { value: null },
  };
  const merged: Record<string, Record<string, unknown>> = {
    ...pluginDefaults,
    ...map,
  };

  await page.addInitScript((m) => {
    const callCounts = new Map<string, number>();
    (window as unknown as Record<string, unknown>).__LUMA_TEST_CALLS__ = [];

    const internals = {
      invoke: async (cmd: string, args: Record<string, unknown> = {}) => {
        const spec = m[cmd];
        const n = callCounts.get(cmd) ?? 0;
        callCounts.set(cmd, n + 1);

        const calls = (
          (window as unknown as Record<string, unknown>).__LUMA_TEST_CALLS__ as
            | TestCall[]
            | undefined
        ) ?? [];
        calls.push({ cmd, args, callNo: n });
        (window as unknown as Record<string, unknown>).__LUMA_TEST_CALLS__ = calls;

        if (!spec) {
          throw new Error(`[mock] no handler for "${cmd}"`);
        }
        if (spec.error !== undefined) {
          throw new Error(String(spec.error));
        }
        return spec.value;
      },
      transformCallback: (_cb: unknown, _once?: boolean) => 1,
      unregisterCallback: () => {},
      convertFileSrc: (p: string) => p,
    };

    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = internals;
    (window as unknown as Record<string, unknown>).__TAURI__ = {
      core: {
        invoke: internals.invoke,
        isTauri: () => true,
      },
    };
  }, merged);
}

/** Read back the recorded `invoke` calls. */
export async function getTestCalls(page: Page): Promise<TestCall[]> {
  return page.evaluate(
    () =>
      (window as unknown as Record<string, unknown>)
        .__LUMA_TEST_CALLS__ as TestCall[],
  );
}
