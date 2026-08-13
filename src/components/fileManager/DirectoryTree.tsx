import { ChevronRight, Folder, FolderOpen } from "lucide-react";
import { useState } from "react";
import type { DirNode } from "../../types/fileManager";
import { formatBytes } from "../../lib/format";

type TreeNodeProps = {
  node: DirNode;
  currentPath: string;
  scanId: string;
  depth: number;
  onNavigate: (path: string) => void;
  onLoadChildren: (path: string) => Promise<DirNode[]>;
};

function TreeNode({
  node,
  currentPath,
  depth,
  onNavigate,
  onLoadChildren,
}: TreeNodeProps) {
  const [expanded, setExpanded] = useState(false);
  const [children, setChildren] = useState<DirNode[] | null>(null);
  const [loading, setLoading] = useState(false);

  const isActive = currentPath === node.path;

  async function handleExpand(e: React.MouseEvent) {
    e.stopPropagation();
    if (!node.hasChildren) return;
    if (!expanded) {
      if (children === null) {
        setLoading(true);
        try {
          const loaded = await onLoadChildren(node.path);
          setChildren(loaded);
        } finally {
          setLoading(false);
        }
      }
      setExpanded(true);
    } else {
      setExpanded(false);
    }
  }

  function handleClick() {
    onNavigate(node.path);
  }

  return (
    <li>
      <div
        className={`fm-tree-node${isActive ? " fm-tree-node--active" : ""}`}
        style={{ paddingLeft: `${12 + depth * 16}px` }}
      >
        {/* Expand/collapse toggle */}
        <button
          type="button"
          className="fm-tree-toggle"
          onClick={handleExpand}
          aria-label={expanded ? "收起" : "展开"}
          disabled={!node.hasChildren}
          tabIndex={-1}
        >
          {node.hasChildren ? (
            loading ? (
              <span className="fm-tree-spinner" aria-hidden />
            ) : (
              <ChevronRight
                size={12}
                style={{
                  transform: expanded ? "rotate(90deg)" : undefined,
                  transition: "transform 0.15s ease",
                }}
              />
            )
          ) : (
            <span style={{ width: 12, display: "inline-block" }} />
          )}
        </button>

        {/* Directory row */}
        <button
          type="button"
          className="fm-tree-label"
          onClick={handleClick}
          title={node.path}
        >
          {isActive ? (
            <FolderOpen size={14} className="fm-tree-icon fm-tree-icon--open" />
          ) : (
            <Folder size={14} className="fm-tree-icon" />
          )}
          <span className="fm-tree-name">{node.name}</span>
          <span className="fm-tree-size">{formatBytes(node.sizeBytes)}</span>
        </button>
      </div>

      {expanded && children && children.length > 0 && (
        <ul className="fm-tree-children">
          {children.map((child) => (
            <TreeNode
              key={child.path}
              node={child}
              currentPath={currentPath}
              scanId=""
              depth={depth + 1}
              onNavigate={onNavigate}
              onLoadChildren={onLoadChildren}
            />
          ))}
        </ul>
      )}
    </li>
  );
}

// ── Root ───────────────────────────────────────────────────────────────────────

type Props = {
  rootDirs: DirNode[];
  currentPath: string;
  scanId: string;
  onNavigate: (path: string) => void;
  onLoadChildren: (path: string) => Promise<DirNode[]>;
};

export function DirectoryTree({
  rootDirs,
  currentPath,
  scanId,
  onNavigate,
  onLoadChildren,
}: Props) {
  if (rootDirs.length === 0) {
    return (
      <div className="fm-tree-empty">
        <Folder size={20} />
        <span>无子目录</span>
      </div>
    );
  }

  return (
    <nav className="fm-tree" aria-label="目录树">
      <ul className="fm-tree-list">
        {rootDirs.map((node) => (
          <TreeNode
            key={node.path}
            node={node}
            currentPath={currentPath}
            scanId={scanId}
            depth={0}
            onNavigate={onNavigate}
            onLoadChildren={onLoadChildren}
          />
        ))}
      </ul>
    </nav>
  );
}
