// All state and data-fetching logic for the file manager view.
// Components stay presentation-only; this hook owns the domain.

import { useCallback, useEffect, useReducer } from "react";
import type {
  BreadcrumbSegment,
  DirFileSort,
  DirNode,
  DirectoryListing,
  FileListView,
} from "../../types/fileManager";
import type { FileEntry } from "../../types/scan";
import { listDirectoryFiles, openPath, revealPath } from "../../lib/tauri";
import { errorMessage } from "../../lib/errors";

// ── State ──────────────────────────────────────────────────────────────────────

type Status = "idle" | "loading" | "ready" | "error";

type State = {
  status: Status;
  error?: string;
  currentPath: string;
  rootPath: string;
  breadcrumbs: BreadcrumbSegment[];
  dirs: DirNode[];
  files: FileEntry[];
  totalFiles: number;
  /** Currently selected file (for preview). */
  selected: FileEntry | null;
  sort: DirFileSort;
  view: FileListView;
  includeHidden: boolean;
  page: number;
  pageSize: number;
};

// ── Actions ────────────────────────────────────────────────────────────────────

type Action =
  | { type: "NAVIGATE"; path: string; breadcrumbs: BreadcrumbSegment[] }
  | { type: "LOADING" }
  | {
      type: "LOADED";
      listing: DirectoryListing;
      path: string;
      breadcrumbs: BreadcrumbSegment[];
    }
  | { type: "ERROR"; error: string }
  | { type: "SELECT_FILE"; file: FileEntry | null }
  | { type: "SET_SORT"; sort: DirFileSort }
  | { type: "SET_VIEW"; view: FileListView }
  | { type: "SET_INCLUDE_HIDDEN"; value: boolean }
  | { type: "SET_PAGE"; page: number };

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case "LOADING":
      return { ...state, status: "loading", error: undefined };

    case "LOADED":
      return {
        ...state,
        status: "ready",
        error: undefined,
        currentPath: action.path,
        breadcrumbs: action.breadcrumbs,
        dirs: action.listing.dirs,
        files: action.listing.files,
        totalFiles: action.listing.totalFiles,
        selected: null,
        page: 0,
      };

    case "ERROR":
      return { ...state, status: "error", error: action.error };

    case "SELECT_FILE":
      return { ...state, selected: action.file };

    case "SET_SORT":
      return { ...state, sort: action.sort, page: 0 };

    case "SET_VIEW":
      return { ...state, view: action.view };

    case "SET_INCLUDE_HIDDEN":
      return { ...state, includeHidden: action.value, page: 0 };

    case "SET_PAGE":
      return { ...state, page: action.page };

    default:
      return state;
  }
}

// ── Breadcrumb builder ─────────────────────────────────────────────────────────

function buildBreadcrumbs(
  rootPath: string,
  targetPath: string,
): BreadcrumbSegment[] {
  // rootPath is the scan root. targetPath must be rootPath or a descendant.
  // We emit one crumb per segment starting at the root.
  const crumbs: BreadcrumbSegment[] = [];

  // Root crumb
  const rootName = rootPath.replace(/[/\\]$/, "").split(/[/\\]/).pop() ?? rootPath;
  crumbs.push({ path: rootPath, name: rootName });

  if (targetPath === rootPath) return crumbs;

  // Relative segments after the root
  const rel = targetPath.slice(rootPath.length).replace(/^[/\\]/, "");
  const segments = rel.split(/[/\\]/).filter(Boolean);

  let accumulated = rootPath;
  for (const seg of segments) {
    accumulated = `${accumulated}/${seg}`;
    crumbs.push({ path: accumulated, name: seg });
  }

  return crumbs;
}

// ── Hook ───────────────────────────────────────────────────────────────────────

const PAGE_SIZE = 100;

export function useFileManager(scanId: string, rootPath: string) {
  const [state, dispatch] = useReducer(reducer, {
    status: "idle",
    currentPath: rootPath,
    rootPath,
    breadcrumbs: buildBreadcrumbs(rootPath, rootPath),
    dirs: [],
    files: [],
    totalFiles: 0,
    selected: null,
    sort: "nameAsc",
    view: "list",
    includeHidden: false,
    page: 0,
    pageSize: PAGE_SIZE,
  });

  // Load a directory. Memoised so callers can put it in deps safely.
  const loadDirectory = useCallback(
    async (path: string, page = 0, overrides?: Partial<State>) => {
      dispatch({ type: "LOADING" });
      try {
        const sort = overrides?.sort ?? state.sort;
        const hidden = overrides?.includeHidden ?? state.includeHidden;
        const listing = await listDirectoryFiles(scanId, path, {
          sort,
          includeHidden: hidden,
          limit: PAGE_SIZE,
          offset: page * PAGE_SIZE,
        });
        const breadcrumbs = buildBreadcrumbs(rootPath, path);
        dispatch({ type: "LOADED", listing, path, breadcrumbs });
      } catch (err) {
        dispatch({ type: "ERROR", error: errorMessage(err) });
      }
    },
    [scanId, rootPath, state.sort, state.includeHidden],
  );

  // Load root on mount
  useEffect(() => {
    void loadDirectory(rootPath);
    // intentionally only on mount / scanId / rootPath changes
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scanId, rootPath]);

  // Navigate into a directory
  const navigate = useCallback(
    (path: string) => void loadDirectory(path),
    [loadDirectory],
  );

  // Sort change: reload current dir with new sort
  const setSort = useCallback(
    (sort: DirFileSort) => {
      dispatch({ type: "SET_SORT", sort });
      void loadDirectory(state.currentPath, 0, { sort });
    },
    [loadDirectory, state.currentPath],
  );

  const setView = useCallback(
    (view: FileListView) => dispatch({ type: "SET_VIEW", view }),
    [],
  );

  const setIncludeHidden = useCallback(
    (value: boolean) => {
      dispatch({ type: "SET_INCLUDE_HIDDEN", value });
      void loadDirectory(state.currentPath, 0, { includeHidden: value });
    },
    [loadDirectory, state.currentPath],
  );

  const setPage = useCallback(
    (page: number) => {
      dispatch({ type: "SET_PAGE", page });
      void loadDirectory(state.currentPath, page);
    },
    [loadDirectory, state.currentPath],
  );

  const selectFile = useCallback(
    (file: FileEntry | null) => dispatch({ type: "SELECT_FILE", file }),
    [],
  );

  const handleReveal = useCallback((path: string) => {
    void revealPath(path).catch(() => {
      // Non-fatal: file may have moved since scan
    });
  }, []);

  const handleOpen = useCallback((path: string) => {
    void openPath(path).catch(() => {});
  }, []);

  const totalPages = Math.max(1, Math.ceil(state.totalFiles / PAGE_SIZE));

  // Expose a reload that refreshes the current directory in place.
  // Used by useFileOps after any mutation.
  const reload = useCallback(() => {
    void loadDirectory(state.currentPath, state.page);
  }, [loadDirectory, state.currentPath, state.page]);

  return {
    ...state,
    totalPages,
    navigate,
    setSort,
    setView,
    setIncludeHidden,
    setPage,
    selectFile,
    handleReveal,
    handleOpen,
    reload,
  };
}

export type FileManagerHook = ReturnType<typeof useFileManager>;
