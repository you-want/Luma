# Search Feature Implementation

## Search v2 Update — 2026-08-17

- Database schema upgraded to v4 with an FTS5 external-content index over file name, path, extension, and category.
- Insert/update/delete triggers keep `files_fts` synchronized; legacy databases run an FTS rebuild during migration.
- Added BM25 relevance sorting with higher weight for file-name matches.
- Added deterministic local parsing for size, recent-year, PDF, Downloads, and project-marker conditions.
- Added a `SearchProvider` abstraction; the Tauri command now uses `LocalSqliteProvider` as the authoritative implementation.
- Added migration, trigger synchronization, Chinese keyword, combined-condition, and 100k-file performance tests.
- 100k debug baseline on macOS: 16.62s indexed insert, 4.52ms keyword search, 32,677,888-byte SQLite database.
- Detailed execution status is maintained in `docs/search-v2-plan.md`.

## Overview

Implemented comprehensive file search functionality with filtering, sorting, and pagination for the Luma file scanner application.

## Date

2026-08-06

## Package

SEARCH-001 through SEARCH-008 (Package C from product roadmap)

## Changes

### Backend (Rust)

**New Types** (`src-tauri/src/models.rs`):
- `SearchRequest` — query params with `query` (optional), `categories`, `extensions`, `size_min`/`size_max`, `modified_after`/`modified_before`, `include_hidden`, `sort`, `page`, `page_size`
- `SearchResponse` — paginated results with `files: Vec<FileEntry>`, `total: usize`, `page: usize`, `page_size: usize`
- `SearchSort` enum — `NameAsc`, `NameDesc`, `SizeAsc`, `SizeDesc`, `ModifiedAsc`, `ModifiedDesc`

**Database** (`src-tauri/src/database.rs`):
- New function `search_files(path: &Path, scan_id: &str, request: &SearchRequest)` with:
  - Name/path LIKE search with proper `%`/`_` escaping
  - Category filter (IN clause)
  - Extension filter (IN clause)
  - Size range filter (BETWEEN)
  - Modified time filter (>=/<)
  - Hidden files toggle
  - Whitelisted ORDER BY (name/size_bytes/modified_at ASC/DESC)
  - Pagination (LIMIT/OFFSET)
  - Separate COUNT query for total
- New index `idx_files_scan_name` on `files(scan_id, name)` for name search performance
- Schema version remains at 2 (index uses `IF NOT EXISTS`, runs every startup)

**Commands** (`src-tauri/src/commands.rs`):
- New Tauri command `search_files` exposing `database::search_files` to frontend

**Tests** (`src-tauri/src/database.rs::tests`):
- `test_search_files` — validates name search, category filter, size range, extension filter, sorting, pagination, and total count

### Frontend (React + TypeScript)

**Types** (`src/types/scan.ts`):
- `SearchRequest`, `SearchResponse`, `SearchSort` (matching Rust models)

**API** (`src/lib/tauri.ts`):
- `searchFiles(scanId: string, request: SearchRequest): Promise<SearchResponse>`

**Component** (`src/components/Search.tsx`):
- Search input with 300ms debounce
- Category multi-select dropdown
- Extension input (comma-separated)
- Size range inputs (min/max MB)
- Modified date filter (after/before date pickers)
- Hidden files toggle
- Sort dropdown (name/size/modified, asc/desc)
- Results table with columns: name, path, category, size, modified
- Pagination controls (prev/next, page N of M)
- Reveal in file manager action per row
- Loading state, empty state, error state
- All labels/messages i18n-enabled

**i18n** (`src/i18n/locales/{zh-CN,en-US}.ts`):
- New `search` namespace with 40+ keys:
  - `title`, `placeholder`, `search`, `filters`, `sort`, `results`
  - `category`, `extension`, `sizeRange`, `modifiedDate`, `includeHidden`
  - `nameAsc`, `nameDesc`, `sizeAsc`, `sizeDesc`, `modifiedAsc`, `modifiedDesc`
  - `name`, `path`, `size`, `modified`, `actions`, `reveal`
  - `page`, `of`, `previous`, `next`
  - `noQuery`, `noResults`, `loading`, `error`
  - `minMB`, `maxMB`, `after`, `before`, `apply`, `clear`
  - Messages and help text

### Verification

**Rust:**
- 23 tests pass (including new `test_search_files`)
- `cargo fmt --check` ✓
- `cargo clippy -D warnings` ✓

**Frontend:**
- 6 tests pass
- Production build clean
- TypeScript strict mode satisfied

## Not Done (Honest Gaps)

- **Manual UI regression** — Search component not yet tested interactively in desktop app (SEARCH-008 manual part)
- **Performance baseline** — 100k baseline recorded; 1M baseline remains pending.
- **Spotlight integration** — macOS provider, canonical-path deduplication, and failure isolation remain pending.

## Next Steps

1. Add multi-provider result metadata and canonical-path deduplication.
2. Add an optional macOS Spotlight provider with local-only fallback.
3. Run desktop UI regression for relevance and natural-language conditions.
4. Record a 1M-file performance baseline.
5. Consider search history or saved searches.

## Files Changed

**New:**
- `src/components/Search.tsx`

**Modified:**
- `src-tauri/src/models.rs` — added SearchRequest/Response/Sort
- `src-tauri/src/database.rs` — added search_files function, idx_files_scan_name index, imports
- `src-tauri/src/commands.rs` — added search_files command
- `src-tauri/src/lib.rs` — registered search_files command
- `src/lib/tauri.ts` — added searchFiles API function
- `src/types/scan.ts` — added SearchRequest/Response/Sort types
- `src/i18n/locales/zh-CN.ts` — added search.* keys
- `src/i18n/locales/en-US.ts` — added search.* keys
- `docs/product-roadmap.md` — marked SEARCH-001~008 as DONE, updated baseline table, added progress record
- `README.md` — added search bullet point to feature list, updated project structure
