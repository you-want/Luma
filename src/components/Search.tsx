import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { FileEntry, SearchRequest, SearchSort } from "../types/scan";
import { searchFiles, revealPath } from "../lib/tauri";
import { formatBytes } from "../lib/format";
import { categoryLabelKey } from "../lib/categories";
import { errorMessage } from "../lib/errors";
import { useSelection } from "../contexts/SelectionContext";

type SearchProps = {
  scanId: string;
  categories: string[];
};

export default function Search({ scanId, categories }: SearchProps) {
  const { t } = useTranslation();
  const selection = useSelection();
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState<string>("");
  const [minSize, setMinSize] = useState("");
  const [maxSize, setMaxSize] = useState("");
  const [sort, setSort] = useState<SearchSort>("relevance");
  const [includeHidden, setIncludeHidden] = useState(false);
  const [results, setResults] = useState<FileEntry[]>([]);
  const [total, setTotal] = useState(0);
  const [offset, setOffset] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const limit = 50;
  // The backend clamps `limit` to this maximum per request (see
  // database::search_files). "Select all" export/copy must therefore page
  // through results rather than asking for everything in one call.
  const MAX_PAGE = 200;

  // The filter half of a search request, shared by the paged view and the
  // fetch-all export/copy paths so they can never drift out of sync.
  const buildBaseRequest = (): Omit<SearchRequest, "limit" | "offset"> => ({
    scanId,
    query: query.trim(),
    category: category || undefined,
    minSize: minSize ? parseInt(minSize, 10) : undefined,
    maxSize: maxSize ? parseInt(maxSize, 10) : undefined,
    includeHidden,
    sort,
  });

  // Fetch every file matching the current filters by paging at the backend's
  // max page size until `total` rows are collected. Used by copy/export in
  // "all" mode, where the frontend does not hold the full result set. The
  // total is re-read each page so a shrinking index cannot loop forever.
  const fetchAllMatching = async (): Promise<FileEntry[]> => {
    const base = buildBaseRequest();
    const collected: FileEntry[] = [];
    let pageOffset = 0;
    for (;;) {
      const response = await searchFiles({
        ...base,
        limit: MAX_PAGE,
        offset: pageOffset,
      });
      collected.push(...response.files);
      pageOffset += response.files.length;
      if (
        response.files.length === 0 ||
        collected.length >= response.total
      ) {
        break;
      }
    }
    return collected;
  };

  const handleSearch = async (newOffset = 0) => {
    setLoading(true);
    setError(null);
    try {
      const response = await searchFiles({
        ...buildBaseRequest(),
        limit,
        offset: newOffset,
      });
      setResults(response.files);
      setTotal(response.total);
      setOffset(newOffset);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setLoading(false);
    }
  };

  const handleReveal = async (path: string) => {
    try {
      await revealPath(path);
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const totalPages = Math.ceil(total / limit);
  const currentPage = Math.floor(offset / limit) + 1;

  // Selection helpers
  const pageFileIds = results.map((f) => f.id);
  const allPageSelected = pageFileIds.length > 0 && pageFileIds.every((id) => selection.isSelected(scanId, id));
  const somePageSelected = pageFileIds.some((id) => selection.isSelected(scanId, id)) && !allPageSelected;

  const toggleSelectAll = () => {
    if (allPageSelected) {
      selection.deselectMultiple(scanId, pageFileIds);
    } else {
      selection.selectMultiple(scanId, pageFileIds);
    }
  };

  const handleCopyPaths = async () => {
    try {
      // In "all" mode the frontend does not hold every match, so page the
      // full set from the backend; otherwise use the selected rows on hand.
      const paths =
        selection.mode === "all"
          ? (await fetchAllMatching()).map((f) => f.path)
          : results
              .filter((f) => selection.isSelected(scanId, f.id))
              .map((f) => f.path);

      await navigator.clipboard.writeText(paths.join("\n"));
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const handleExportList = async () => {
    try {
      const files =
        selection.mode === "all"
          ? await fetchAllMatching()
          : results.filter((f) => selection.isSelected(scanId, f.id));

      // Build CSV content
      const header = "Name,Path,Category,Size (bytes),Modified\n";
      const rows = files.map((f) => {
        const name = f.name.replace(/"/g, '""');
        const path = f.path.replace(/"/g, '""');
        const category = f.category;
        const size = f.sizeBytes;
        const modified = f.modifiedAt || "";
        return `"${name}","${path}","${category}",${size},"${modified}"`;
      }).join("\n");

      const csv = header + rows;
      const blob = new Blob([csv], { type: "text/csv;charset=utf-8;" });
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = `luma-export-${Date.now()}.csv`;
      link.click();
      URL.revokeObjectURL(url);
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  return (
    <div className="search-panel">
      <h2>{t("search.title")}</h2>

      <div className="search-filters">
        <input
          type="text"
          placeholder={t("search.queryPlaceholder")}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && handleSearch(0)}
          className="search-input"
        />

        <select
          value={category}
          onChange={(e) => setCategory(e.target.value)}
          className="search-select"
        >
          <option value="">{t("search.allCategories")}</option>
          {categories.map((cat) => (
            <option key={cat} value={cat}>
              {t(categoryLabelKey(cat))}
            </option>
          ))}
        </select>

        <select
          value={sort}
          onChange={(e) => setSort(e.target.value as SearchSort)}
          className="search-select"
        >
          <option value="relevance">{t("search.sort.relevance")}</option>
          <option value="sizeDesc">{t("search.sort.sizeDesc")}</option>
          <option value="sizeAsc">{t("search.sort.sizeAsc")}</option>
          <option value="nameAsc">{t("search.sort.nameAsc")}</option>
          <option value="nameDesc">{t("search.sort.nameDesc")}</option>
          <option value="modifiedDesc">{t("search.sort.modifiedDesc")}</option>
          <option value="modifiedAsc">{t("search.sort.modifiedAsc")}</option>
        </select>

        <div className="search-size-filters">
          <input
            type="number"
            placeholder={t("search.minSize")}
            value={minSize}
            onChange={(e) => setMinSize(e.target.value)}
            className="search-input-number"
            min="0"
          />
          <span>—</span>
          <input
            type="number"
            placeholder={t("search.maxSize")}
            value={maxSize}
            onChange={(e) => setMaxSize(e.target.value)}
            className="search-input-number"
            min="0"
          />
        </div>

        <label className="search-checkbox">
          <input
            type="checkbox"
            checked={includeHidden}
            onChange={(e) => setIncludeHidden(e.target.checked)}
          />
          {t("search.includeHidden")}
        </label>

        <button onClick={() => handleSearch(0)} disabled={loading} className="button-primary">
          {loading ? t("search.searching") : t("search.search")}
        </button>
      </div>

      {error && <div className="error-message">{error}</div>}

      {selection.count > 0 && (
        <div className="selection-toolbar">
          <span className="selection-count">
            {selection.mode === "all"
              ? t("selection.selectedAll")
              : t("selection.selectedCount", { count: selection.count })}
          </span>
          <div className="selection-actions">
            <button onClick={handleCopyPaths} className="button-secondary">
              {t("selection.copyPaths")}
            </button>
            <button onClick={handleExportList} className="button-secondary">
              {t("selection.exportList")}
            </button>
            <button onClick={selection.clear} className="button-secondary">
              {t("selection.clearSelection")}
            </button>
          </div>
        </div>
      )}

      {results.length > 0 && (
        <>
          <div className="search-results-header">
            <span>
              {t("search.resultsCount", { count: total, from: offset + 1, to: Math.min(offset + limit, total) })}
            </span>
          </div>

          <table className="file-table">
            <thead>
              <tr>
                <th style={{ width: "40px" }}>
                  <input
                    type="checkbox"
                    checked={allPageSelected}
                    onChange={toggleSelectAll}
                    aria-label={allPageSelected ? t("selection.deselectAll") : t("selection.selectAll")}
                    ref={(el) => {
                      if (el) el.indeterminate = somePageSelected;
                    }}
                  />
                </th>
                <th>{t("search.table.name")}</th>
                <th>{t("search.table.category")}</th>
                <th>{t("search.table.size")}</th>
                <th>{t("search.table.path")}</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {results.map((file, index) => (
                <tr key={`${file.path}-${index}`}>
                  <td>
                    <input
                      type="checkbox"
                      checked={selection.isSelected(scanId, file.id)}
                      onChange={() => selection.toggle(scanId, file.id)}
                      aria-label={`Select ${file.name}`}
                    />
                  </td>
                  <td>{file.name}</td>
                  <td>{t(categoryLabelKey(file.category))}</td>
                  <td>{formatBytes(file.sizeBytes)}</td>
                  <td className="file-path">{file.path}</td>
                  <td>
                    <button onClick={() => handleReveal(file.path)} className="button-link">
                      {t("common.reveal")}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>

          {totalPages > 1 && (
            <div className="pagination">
              <button
                onClick={() => handleSearch(offset - limit)}
                disabled={offset === 0 || loading}
                className="button-secondary"
              >
                {t("search.previous")}
              </button>
              <span>
                {t("search.pageInfo", { current: currentPage, total: totalPages })}
              </span>
              <button
                onClick={() => handleSearch(offset + limit)}
                disabled={offset + limit >= total || loading}
                className="button-secondary"
              >
                {t("search.next")}
              </button>
            </div>
          )}
        </>
      )}

      {!loading && results.length === 0 && total === 0 && offset === 0 && query && (
        <p className="empty-state">{t("search.noResults")}</p>
      )}
    </div>
  );
}
