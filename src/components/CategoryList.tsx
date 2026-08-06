import { useTranslation } from "react-i18next";
import { categoryColor, categoryLabelKey } from "../lib/categories";
import { formatBytes } from "../lib/format";
import type { CategorySummary } from "../types/scan";

export function CategoryList({
  categories,
  totalBytes,
}: {
  categories: CategorySummary[];
  totalBytes: number;
}) {
  const { t } = useTranslation();
  // Segments narrower than this read as slivers; clamp so every present
  // category stays visible in the bar without distorting the large ones.
  const visible = categories.filter((item) => item.sizeBytes > 0);

  return (
    <section className="storage-panel" aria-labelledby="category-title">
      <div className="section-heading result-heading">
        <h2 id="category-title">{t("category.title")}</h2>
        <span className="scan-time">{t("category.count", { count: visible.length })}</span>
      </div>

      <div
        className="storage-bar"
        role="img"
        aria-label={t("category.barLabel", { total: formatBytes(totalBytes) })}
      >
        {visible.map((item) => {
          const percentage = totalBytes ? (item.sizeBytes / totalBytes) * 100 : 0;
          return (
            <div
              className="storage-seg"
              key={item.category}
              style={{
                flex: `${Math.max(percentage, 0.6)} 0 0`,
                background: categoryColor(item.category),
              }}
              title={`${t(categoryLabelKey(item.category))} · ${formatBytes(item.sizeBytes)}`}
            />
          );
        })}
      </div>

      <div className="storage-legend">
        {visible.map((item) => (
          <div className="legend-item" key={item.category}>
            <span
              className="legend-dot"
              style={{ background: categoryColor(item.category) }}
            />
            <span className="legend-label">{t(categoryLabelKey(item.category))}</span>
            <span className="legend-size">{formatBytes(item.sizeBytes)}</span>
          </div>
        ))}
      </div>
    </section>
  );
}
