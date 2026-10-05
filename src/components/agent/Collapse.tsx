import { useState, type ReactNode } from 'react';

import { ChevronRightIcon } from '@/components/icons';
import { cn } from '@/lib/cn';

export interface CollapseProps {
  title: ReactNode;
  /** 标题右侧的补充信息（如「N 条」）。 */
  hint?: ReactNode;
  defaultOpen?: boolean;
  /** 折叠时是否保留内容在 DOM（默认不保留，展开才挂载）。 */
  keepMounted?: boolean;
  className?: string;
  children: ReactNode;
}

/**
 * 折叠容器。
 *
 * 此前项目里折叠有两种做法并存：原生 details/summary（SearchPage、SettingsPage
 * 等 4 处）与手写 state + Chevron 图标（ClaimTraceCard 的 TraceStep、
 * ActivityPanel 的 ActivityItem）。样式因此不一致，且原生 details 无法在标题里
 * 挂徽章 / 计数 / 操作按钮。这里统一到手写版本。
 */
export function Collapse({
  title,
  hint,
  defaultOpen = false,
  keepMounted = false,
  className,
  children,
}: CollapseProps) {
  const [open, setOpen] = useState(defaultOpen);

  return (
    <div className={cn('overflow-hidden rounded-lg border border-line bg-surface', className)}>
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
        className="flex w-full items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-elevated/50"
      >
        <ChevronRightIcon
          className={cn(
            'h-3.5 w-3.5 shrink-0 text-muted transition-transform',
            open && 'rotate-90',
          )}
        />
        <span className="min-w-0 flex-1 truncate text-meta font-medium text-muted">
          {title}
        </span>
        {hint ? <span className="shrink-0 font-mono text-[10px] text-muted/80">{hint}</span> : null}
      </button>
      {open || keepMounted ? (
        <div className="border-t border-line px-3 py-2.5">{children}</div>
      ) : null}
    </div>
  );
}
