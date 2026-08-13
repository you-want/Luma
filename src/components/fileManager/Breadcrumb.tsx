import { ChevronRight } from "lucide-react";
import type { BreadcrumbSegment } from "../../types/fileManager";

type Props = {
  segments: BreadcrumbSegment[];
  onNavigate: (path: string) => void;
};

export function Breadcrumb({ segments, onNavigate }: Props) {
  return (
    <nav className="fm-breadcrumb" aria-label="目录路径">
      <ol>
        {segments.map((seg, i) => {
          const isLast = i === segments.length - 1;
          return (
            <li key={seg.path}>
              {isLast ? (
                <span className="fm-breadcrumb-current" aria-current="page">
                  {seg.name}
                </span>
              ) : (
                <>
                  <button
                    type="button"
                    className="fm-breadcrumb-link"
                    onClick={() => onNavigate(seg.path)}
                    title={seg.path}
                  >
                    {seg.name}
                  </button>
                  <ChevronRight size={12} className="fm-breadcrumb-sep" aria-hidden />
                </>
              )}
            </li>
          );
        })}
      </ol>
    </nav>
  );
}
