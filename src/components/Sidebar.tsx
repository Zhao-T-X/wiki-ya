import type { ComponentType } from 'react';
import { Link, useLocation } from 'react-router-dom';

import {
  AskIcon,
  ClockIcon,
  DatabaseIcon,
  GraphIcon,
  InboxIcon,
  KnowledgeIcon,
  ResearchIcon,
  ReviewIcon,
  SearchIcon,
  SettingsIcon,
  type IconProps,
} from '@/components/icons';
import { cn } from '@/lib/cn';
import { useUiStore, type ModuleId } from '@/stores/ui';

interface NavItem {
  id: ModuleId;
  to: string;
  label: string;
  hint: string;
  icon: ComponentType<IconProps>;
  /** 归属该模块的路由前缀（如 /claims、/documents 均属 Knowledge）。 */
  match: string[];
}

/**
 * 一级导航：严格对应 PRD §4 的 8 个模块。
 * 不暴露 Ontology / Agent / Context Runtime。
 */
const NAV_ITEMS: NavItem[] = [
  { id: 'inbox', to: '/inbox', label: 'Inbox', hint: '捕获', icon: InboxIcon, match: ['/inbox'] },
  {
    id: 'knowledge',
    to: '/knowledge',
    label: 'Knowledge',
    hint: '知识',
    icon: KnowledgeIcon,
    match: ['/knowledge', '/claims', '/documents'],
  },
  { id: 'search', to: '/search', label: 'Search', hint: '检索', icon: SearchIcon, match: ['/search'] },
  { id: 'review', to: '/review', label: 'Review', hint: '审核', icon: ReviewIcon, match: ['/review'] },
  { id: 'ask', to: '/ask', label: 'Ask', hint: '问答', icon: AskIcon, match: ['/ask'] },
  { id: 'research', to: '/research', label: 'Research', hint: '研究', icon: ResearchIcon, match: ['/research'] },
  { id: 'graph', to: '/graph', label: 'Graph', hint: '图谱', icon: GraphIcon, match: ['/graph'] },
  { id: 'timeline', to: '/timeline', label: 'Timeline', hint: '时间线', icon: ClockIcon, match: ['/timeline'] },
  { id: 'migration', to: '/migration', label: 'Migration', hint: '迁移', icon: DatabaseIcon, match: ['/migration'] },
  { id: 'settings', to: '/settings', label: 'Settings', hint: '设置', icon: SettingsIcon, match: ['/settings'] },
];

export function Sidebar() {
  const location = useLocation();
  const openCommandPalette = useUiStore((state) => state.openCommandPalette);

  const isActive = (item: NavItem): boolean =>
    item.match.some(
      (prefix) => location.pathname === prefix || location.pathname.startsWith(`${prefix}/`),
    );

  return (
    <aside className="flex h-full w-60 shrink-0 flex-col border-r border-line bg-surface">
      <div className="flex items-center gap-2.5 px-5 py-5">
        <span className="flex h-7 w-7 items-center justify-center rounded-lg bg-accent/15 text-accent">
          <KnowledgeIcon className="h-4 w-4" />
        </span>
        <div className="leading-tight">
          <p className="text-sm font-semibold text-ink">wiki-ya</p>
          <p className="text-[10px] tracking-wide text-muted">local-first knowledge</p>
        </div>
      </div>

      <nav className="flex-1 space-y-0.5 px-3">
        {NAV_ITEMS.map((item) => {
          const active = isActive(item);
          const Icon = item.icon;
          return (
            <Link
              key={item.id}
              to={item.to}
              aria-current={active ? 'page' : undefined}
              className={cn(
                'group flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm transition-colors',
                active ? 'bg-elevated text-ink' : 'text-muted hover:bg-elevated/60 hover:text-ink',
              )}
            >
              <Icon className={cn('h-4 w-4 shrink-0', active ? 'text-accent' : 'text-muted group-hover:text-ink')} />
              <span className="flex-1 font-medium">{item.label}</span>
              <span className="text-[10px] text-muted/70">{item.hint}</span>
            </Link>
          );
        })}
      </nav>

      <div className="border-t border-line px-3 py-3">
        <button
          type="button"
          onClick={openCommandPalette}
          className="flex w-full items-center justify-between rounded-lg px-3 py-2 text-xs text-muted transition-colors hover:bg-elevated hover:text-ink"
        >
          <span className="flex items-center gap-2">
            <SearchIcon className="h-3.5 w-3.5" />
            全局搜索
          </span>
          <kbd className="rounded border border-line bg-canvas px-1.5 py-0.5 font-mono text-[10px] text-muted">
            ⌘K
          </kbd>
        </button>
      </div>
    </aside>
  );
}
