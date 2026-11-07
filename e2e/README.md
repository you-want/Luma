# E2E tests (Playwright)

End-to-end tests that drive the React app **in a real Chromium** against the
plain Vite dev server — no Tauri shell. Since the browser has no native
backend, `window.__TAURI__` / `__TAURI_INTERNALS__` are stubbed so every
`invoke` from `src/lib/tauri.ts` resolves against JSON fixtures instead of
the Rust core.

## Run

```bash
pnpm test:e2e            # all e2e specs
pnpm test:e2e:cleaner    # cleaner (磁盘管家) spec only
pnpm test:e2e:scan       # scan / analysis spec only
```

The Playwright config (`playwright.config.ts`) starts the Vite dev server
automatically on port 1030 (the Tauri dev port) unless one is already
running.

## How the mock works

- `e2e/tauriMock.ts` → `installTauriMock(page, handlers)` installs a
  deterministic `__TAURI_INTERNALS__` before the page's JS runs, so the app's
  mount-time calls (`getLatestScan`, event listeners, etc.) resolve against
  your fixtures.
- `handlers` is a plain map: command name → `{ value }` or `{ error }`.
  Unknown commands reject, so error paths surface as testable alerts.
- `getTestCalls(page)` returns the list of `invoke` calls the app made, in
  order — assert the command sequence without reaching into the DOM.

Fixture data must match the real types in `src/types/scan.ts` (e.g.
`FileEntry` has `id`, `path`, `name`, `sizeBytes`, …).

## Scope / limits

- Tests UI logic: tab gating, overview/error/empty states, confirm-dialog
  copy, panel expansion. They do **not** exercise the native backend,
  the OS trash, or real filesystem effects.
- The backend's logic is covered separately by the Rust unit + command-layer
  integration tests (`src-tauri`, `cargo test`).
