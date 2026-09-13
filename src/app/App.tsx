import { useEffect } from 'react';
import { Outlet, useLocation } from 'react-router-dom';

import { CommandPalette } from '@/components/CommandPalette';
import { Sidebar } from '@/components/Sidebar';
import { AlertIcon } from '@/components/icons';
import { isTauri } from '@/lib/api';
import { useUiStore, type ModuleId } from '@/stores/ui';

/** 路由首段 → 一级模块（/claims、/documents 归属 Knowledge）。 */
const SEGMENT_TO_MODULE: Record<string, ModuleId> = {
  inbox: 'inbox',
  knowledge: 'knowledge',
  claims: 'knowledge',
  documents: 'knowledge',
  search: 'search',
  review: 'review',
  ask: 'ask',
  research: 'research',
  graph: 'graph',
  timeline: 'timeline',
  migration: 'migration',
  settings: 'settings',
};

/**
 * 根布局：左侧固定侧边栏 + 主内容区。
 * 同时负责主题应用、⌘K 快捷键、模块同步与「浏览器预览模式」提示条。
 */
export function App() {
  const theme = useUiStore((state) => state.theme);
  const setActiveModule = useUiStore((state) => state.setActiveModule);
  const toggleCommandPalette = useUiStore((state) => state.toggleCommandPalette);
  const location = useLocation();

  useEffect(() => {
    const root = document.documentElement;
    root.classList.toggle('dark', theme === 'dark');
    root.style.colorScheme = theme;
  }, [theme]);

  useEffect(() => {
    const segment = location.pathname.split('/')[1] ?? '';
    const module = SEGMENT_TO_MODULE[segment];
    if (module) setActiveModule(module);
  }, [location.pathname, setActiveModule]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        toggleCommandPalette();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [toggleCommandPalette]);

  return (
    <div className="flex h-full overflow-hidden bg-canvas text-ink">
      <Sidebar />
      <main className="h-full min-w-0 flex-1 overflow-y-auto">
        {!isTauri() ? <PreviewBanner /> : null}
        <div className="mx-auto w-full max-w-[1180px] px-8 py-8">
          <Outlet />
        </div>
      </main>
      <CommandPalette />
    </div>
  );
}

/** 浏览器预览模式提示：诚实告知数据不可用，避免误以为功能损坏。 */
function PreviewBanner() {
  return (
    <div className="sticky top-0 z-40 flex items-center gap-2 border-b border-warn/25 bg-warn/10 px-8 py-2 text-xs text-warn backdrop-blur">
      <AlertIcon className="h-3.5 w-3.5 shrink-0" />
      <span>当前运行在浏览器预览模式，数据不可用。请使用 `pnpm tauri:dev` 启动桌面应用。</span>
    </div>
  );
}
