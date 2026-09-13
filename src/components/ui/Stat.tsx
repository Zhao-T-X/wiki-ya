import type { ReactNode } from 'react';

import { cn } from '@/lib/cn';
import type { Tone } from '@/lib/status';

const VALUE_TONES: Record<Tone, string> = {
  neutral: 'text-ink',
  accent: 'text-accent',
  ok: 'text-ok',
  warn: 'text-warn',
  danger: 'text-danger',
};

export interface StatProps {
  label: string;
  value: ReactNode;
  hint?: string;
  tone?: Tone;
  /** 传入即渲染为可点击卡片（Knowledge Health 指标跳转 Review）。 */
  onClick?: () => void;
  className?: string;
}

const SHELL = 'rounded-xl border border-line bg-surface px-4 py-3 text-left transition-colors';

export function Stat({ label, value, hint, tone = 'neutral', onClick, className }: StatProps) {
  const body = (
    <>
      <p className="text-[11px] font-medium uppercase tracking-wide text-muted">{label}</p>
      <p className={cn('mt-1.5 text-2xl font-semibold tabular-nums', VALUE_TONES[tone])}>{value}</p>
      {hint ? <p className="mt-1 text-[11px] text-muted">{hint}</p> : null}
    </>
  );

  if (onClick) {
    return (
      <button
        type="button"
        onClick={onClick}
        className={cn(SHELL, 'group hover:border-accent/40 hover:bg-elevated', className)}
      >
        {body}
      </button>
    );
  }

  return <div className={cn(SHELL, className)}>{body}</div>;
}
