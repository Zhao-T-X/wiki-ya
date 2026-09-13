import type { ReactNode } from 'react';

import { AlertIcon } from '@/components/icons';
import { cn } from '@/lib/cn';
import type { WikiError } from '@/lib/api';

export interface ErrorNoticeProps {
  /** 可省略：此时只渲染 children 自定义文案。 */
  error?: WikiError | null;
  /** 覆盖默认文案（例如 CONFLICT 的友好提示）。 */
  children?: ReactNode;
  tone?: 'danger' | 'warn';
  className?: string;
}

export function ErrorNotice({ error, children, tone = 'danger', className }: ErrorNoticeProps) {
  if (!error && !children) return null;

  const tones = {
    danger: 'border-danger/30 bg-danger/10 text-danger',
    warn: 'border-warn/30 bg-warn/10 text-warn',
  } as const;

  return (
    <div className={cn('flex items-start gap-2 rounded-lg border px-3 py-2 text-xs', tones[tone], className)}>
      <AlertIcon className="mt-0.5 h-3.5 w-3.5 shrink-0" />
      <div className="min-w-0 leading-relaxed">
        {children ?? error?.message}
        {error && children ? <span className="ml-1 opacity-70">（{error.code}）</span> : null}
      </div>
    </div>
  );
}
