import { create } from 'zustand';

/**
 * 全局 UI 状态。
 *
 * 技术设计文档 §58：Zustand **只**放 UI / Session 状态，
 * 绝不把 Knowledge Database 放进来——数据库 Source of Truth 永远是 Rust Domain。
 */

export type ModuleId =
  | 'inbox'
  | 'knowledge'
  | 'search'
  | 'review'
  | 'ask'
  | 'research'
  | 'graph'
  | 'timeline'
  | 'migration'
  | 'settings';

export type Theme = 'dark' | 'light';

interface UiState {
  /** 当前一级模块（由路由同步）。 */
  activeModule: ModuleId;
  commandPaletteOpen: boolean;
  theme: Theme;
  /** Review 页筛选（关系类型）；由 Knowledge Health 卡片点击写入。 */
  reviewFilter: string | null;

  setActiveModule: (module: ModuleId) => void;
  openCommandPalette: () => void;
  closeCommandPalette: () => void;
  toggleCommandPalette: () => void;
  setTheme: (theme: Theme) => void;
  setReviewFilter: (filter: string | null) => void;
}

export const useUiStore = create<UiState>((set) => ({
  activeModule: 'inbox',
  commandPaletteOpen: false,
  theme: 'dark',
  reviewFilter: null,

  setActiveModule: (module) => set({ activeModule: module }),
  openCommandPalette: () => set({ commandPaletteOpen: true }),
  closeCommandPalette: () => set({ commandPaletteOpen: false }),
  toggleCommandPalette: () => set((state) => ({ commandPaletteOpen: !state.commandPaletteOpen })),
  setTheme: (theme) => set({ theme }),
  setReviewFilter: (filter) => set({ reviewFilter: filter }),
}));
