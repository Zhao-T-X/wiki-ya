import { useState, type ComponentType } from 'react';
import { Link, useLocation } from 'react-router-dom';

import {
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
import { list_review_items } from '@/lib/api';
import { cn } from '@/lib/cn';
import { useAsyncData } from '@/lib/hooks';
import { useUiStore, type ModuleId } from '@/stores/ui';

interface NavItem {
  id: ModuleId;
  to: string;
  label: string;
  /** 归属该模块的路由前缀（如 /claims、/documents 均属 Knowledge）。 */
  match: string[];
  icon: ComponentType<IconProps>;
}

/**
 * 一级导航（UX 重构后）：
 *
 * 用户只需要记住两个动作——「我要记东西」（Capture）与「我要找东西」（Search）。
 * 因此导航只保留 4 个日常模块；Ask / Research / Graph / Timeline / Migration
 * 这些低频或内部能力收进「更多」，避免 10 个平级入口逼用户做选择。
 */
const PRIMARY_ITEMS: NavItem[] = [
  {
    id: 'home',
    to: '/home',
    label: 'Home',
    match: ['/home'],
    icon: InboxIcon,
  },
  {
    id: 'knowledge',
    to: '/knowledge',
    label: 'Knowledge',
    match: ['/knowledge', '/claims', '/documents'],
    icon: KnowledgeIcon,
  },
  { id: 'review', to: '/review', label: 'Review', match: ['/review'], icon: ReviewIcon },
  { id: 'search', to: '/search', label: 'Search', match: ['/search'], icon: SearchIcon },
];

/** 低频 / 进阶能力：默认折叠，避免挤压日常路径。Ask 已并入 Search。 */
const MORE_ITEMS: NavItem[] = [
  { id: 'research', to: '/research', label: 'Research', match: ['/research'], icon: ResearchIcon },
  { id: 'graph', to: '/graph', label: 'Graph', match: ['/graph'], icon: GraphIcon },
  { id: 'timeline', to: '/timeline', label: 'Timeline', match: ['/timeline'], icon: ClockIcon },
  { id: 'migration', to: '/migration', label: 'Migration', match: ['/migration'], icon: DatabaseIcon },
];

export function Sidebar() {
  const location = useLocation();
  const openCommandPalette = useUiStore((state) => state.openCommandPalette);

  // Review 不是"一个页面"，而是系统的待办：把待处理数量常驻在导航上。
  const pending = useAsyncData(() => list_review_items({ limit: 50 }), []);
  const pendingCount = pending.data?.length ?? 0;

  const isActive = (item: NavItem): boolean =>
    item.match.some(
      (prefix) => location.pathname === prefix || location.pathname.startsWith(`${prefix}/`),
    );

  // 当前路由落在「更多」里时自动展开，否则保持折叠。
  const inMore = MORE_ITEMS.some(isActive);
  const [moreOpen, setMoreOpen] = useState(inMore);

  const renderItem = (item: NavItem) => {
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
        <Icon
          className={cn('h-4 w-4 shrink-0', active ? 'text-accent' : 'text-muted group-hover:text-ink')}
        />
        <span className="flex-1 font-medium">{item.label}</span>
        {item.id === 'review' && pendingCount > 0 ? (
          <span className="rounded-full bg-warn/20 px-1.5 py-0.5 text-[10px] font-medium text-warn">
            {pendingCount}
          </span>
        ) : null}
      </Link>
    );
  };

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

      {/* 主入口：把东西丢进来。这是整个产品最该被看见的动作。 */}
      <div className="px-3 pb-3">
        <Link
          to="/home"
          className="flex items-center justify-center gap-2 rounded-lg bg-accent/90 px-3 py-2.5 text-sm font-medium text-canvas transition-colors hover:bg-accent"
        >
          <InboxIcon className="h-4 w-4" />
          Capture
        </Link>
      </div>

      <nav className="flex-1 space-y-0.5 px-3">
        {PRIMARY_ITEMS.filter((item) => item.id !== 'home').map(renderItem)}
      </nav>

      <div className="space-y-2 px-3 pb-2">
        <button
          type="button"
          onClick={() => setMoreOpen((value) => !value)}
          aria-expanded={moreOpen}
          className="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-xs text-muted transition-colors hover:bg-elevated/60 hover:text-ink"
        >
          <span className="flex-1 text-left font-medium">更多</span>
          <span className={cn('text-[10px] transition-transform', moreOpen ? 'rotate-90' : '')}>▸</span>
        </button>
        {moreOpen ? <div className="space-y-0.5">{MORE_ITEMS.map(renderItem)}</div> : null}
      </div>

      <div className="border-t border-line px-3 py-3">
        <div className="flex items-center gap-1">
          <Link
            to="/settings"
            aria-current={location.pathname.startsWith('/settings') ? 'page' : undefined}
            className={cn(
              'flex flex-1 items-center gap-2.5 rounded-lg px-3 py-2 text-sm transition-colors',
              location.pathname.startsWith('/settings')
                ? 'bg-elevated text-ink'
                : 'text-muted hover:bg-elevated/60 hover:text-ink',
            )}
          >
            <SettingsIcon className="h-4 w-4 shrink-0" />
            <span className="font-medium">Settings</span>
          </Link>
          <button
            type="button"
            onClick={openCommandPalette}
            title="全局搜索（⌘K）"
            className="flex items-center gap-1.5 rounded-lg px-2.5 py-2 text-xs text-muted transition-colors hover:bg-elevated hover:text-ink"
          >
            <SearchIcon className="h-3.5 w-3.5" />
            <kbd className="rounded border border-line bg-canvas px-1 py-0.5 font-mono text-[10px] text-muted">
              ⌘K
            </kbd>
          </button>
        </div>
      </div>
    </aside>
  );
}
