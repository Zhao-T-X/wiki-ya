import type { ReactNode } from 'react';

import { cn } from '@/lib/cn';

/**
 * 分隔线。
 *
 * PR-07 的视觉原则里，「轻分割」替代「每个区块都画完整边框」是核心手法之一，
 * 所以这个原语此前缺失。它比 `<hr>` 好的地方：两端内缩、方向可选、语义正确。
 */
export function Separator({
  className,
  orientation = 'horizontal',
  label,
}: {
  className?: string;
  orientation?: 'horizontal' | 'vertical';
  /** 可选文字标签（垂直分隔线上方/下方的说明）。 */
  label?: ReactNode;
}) {
  if (orientation === 'vertical') {
    return (
      <span
        role="separator"
        aria-orientation="vertical"
        className={cn('inline-block h-full w-px shrink-0 bg-line', className)}
      />
    );
  }
  if (label) {
    return (
      <div className={cn('flex items-center gap-3', className)}>
        <span className="h-px flex-1 bg-line" />
        <span className="text-[11px] uppercase tracking-wider text-muted">{label}</span>
        <span className="h-px flex-1 bg-line" />
      </div>
    );
  }
  return <hr className={cn('border-line', className)} />;
}
