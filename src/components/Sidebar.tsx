// Left navigation sidebar: the app's primary IA. Replaces the old top tab bar
// so workspace tools (总览/分析/文件/整理), machine tools (磁盘管家/会话) and
// system tools (操作记录/设置) can coexist without crowding a header row.
//
// A11y: vertical tablist — arrow keys move between enabled items, matching the
// interaction the previous horizontal tab bar provided.

import { useTranslation } from "react-i18next";
import {
  BarChart2,
  Bot,
  FolderOpen,
  HardDrive,
  History,
  LayoutDashboard,
  Settings,
  Wand2,
} from "lucide-react";
import type { AppView } from "../types/navigation";
import { NAV_GROUPS } from "../types/navigation";

const VIEW_ICONS: Record<AppView, typeof LayoutDashboard> = {
  overview: LayoutDashboard,
  analysis: BarChart2,
  files: FolderOpen,
  organize: Wand2,
  cleaner: HardDrive,
  sessions: Bot,
  history: History,
  settings: Settings,
};

type SidebarProps = {
  activeView: AppView;
  /** True while no scan result is available — workspace views stay gated. */
  viewsDisabled: boolean;
  onNavigate: (view: AppView) => void;
};

export function Sidebar({ activeView, viewsDisabled, onNavigate }: SidebarProps) {
  const { t } = useTranslation();

  const isDisabled = (view: AppView) =>
    (view === "analysis" || view === "files" || view === "organize") && viewsDisabled;

  function handleKeyDown(event: React.KeyboardEvent) {
    const keys = ["ArrowUp", "ArrowDown", "Home", "End"];
    if (!keys.includes(event.key)) return;
    event.preventDefault();
    const enabled = NAV_GROUPS.flatMap((group) => group.views).filter((v) => !isDisabled(v));
    if (enabled.length === 0) return;
    const current = enabled.includes(activeView) ? enabled.indexOf(activeView) : 0;
    const next = event.key === "Home" ? enabled[0]
      : event.key === "End" ? enabled[enabled.length - 1]
      : enabled[(current + (event.key === "ArrowDown" ? 1 : enabled.length - 1)) % enabled.length];
    onNavigate(next);
    document.getElementById(`workspace-tab-${next}`)?.focus();
  }

  return (
    <aside className="app-sidebar">
      <div className="app-brand">
        <img className="brand-mark" src="/luma-logo.svg" alt="" />
        <div className="brand-copy">
          <strong>{t("app.name")}</strong>
          <span>{t("app.tagline")}</span>
        </div>
      </div>

      <nav className="app-sidebar-nav" role="tablist" aria-orientation="vertical"
        aria-label={t("workspace.views")}>
        {NAV_GROUPS.map((group) => (
          <div className="app-nav-group" key={group.key}>
            <p className="app-nav-group-label" id={`nav-group-${group.key}`}>
              {t(`nav.groups.${group.key}`)}
            </p>
            {group.views.map((view) => {
              const Icon = VIEW_ICONS[view];
              const disabled = isDisabled(view);
              return (
                <button key={view} id={`workspace-tab-${view}`} type="button" role="tab"
                  aria-selected={activeView === view}
                  aria-controls={`workspace-panel-${view}`}
                  tabIndex={activeView === view ? 0 : -1} disabled={disabled}
                  className="app-nav-item"
                  title={disabled ? t("workspace.scanFirst") : undefined}
                  onKeyDown={handleKeyDown}
                  onClick={() => onNavigate(view)}>
                  <Icon size={15} aria-hidden="true" />
                  <span>{t(`nav.views.${view}`)}</span>
                </button>
              );
            })}
          </div>
        ))}
      </nav>

      <p className="app-sidebar-foot">
        <span className="sidebar-privacy" aria-hidden="true" />
        {t("app.privacyNote")}
      </p>
    </aside>
  );
}
