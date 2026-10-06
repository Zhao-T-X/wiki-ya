import { cn } from '@/lib/cn';

export interface CitationProps {
  /** 来源序号（对应 `[n]`）。 */
  index: number;
  /** 点击跳转（正文角标联动来源卡片）。不传则为静态脚注。 */
  onClick?: () => void;
  /** 被正文点亮时的强调态。 */
  active?: boolean;
  className?: string;
}

/**
 * 引用角标（任务书 PR-07.1 T5）。
 *
 * 把 `[n]` 统一成真正的「脚注」视觉：小字号、弱背景、上标对齐，而不是一个
 * 普通 UI button。两处复用：
 * - `MarkdownContent` 正文里的 `[n]`（可点击，传 `onClick`）
 * - `SourceCard` 头部 / 预览抽屉的 `[n]` 序号（静态，仅展示）
 *
 * 点击后的「滚动 + 高亮对应来源」由调用方通过 `onClick` 实现，这里只负责呈现。
 */
export function Citation({ index, onClick, active, className }: CitationProps) {
  const cls = cn(
    'mx-0.5 inline-flex h-[14px] min-w-[14px] items-center justify-center rounded px-1 align-super font-mono text-[10px] leading-none transition-colors',
    active ? 'bg-accent/25 text-accent ring-1 ring-accent/40' : 'bg-accent/10 text-accent',
    onClick ? 'cursor-pointer hover:bg-accent/20' : 'cursor-default',
    className,
  );

  if (onClick) {
    return (
      <button type="button" onClick={onClick} title={`跳到来源 [${index}]`} className={cls}>
        {index}
      </button>
    );
  }
  return <span className={cls}>{index}</span>;
}
