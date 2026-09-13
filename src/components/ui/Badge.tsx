import type { ReactNode } from 'react';

import { cn } from '@/lib/cn';
import { statusLabel, statusTone, type Tone } from '@/lib/status';

const TONES: Record<Tone, string> = {
  neutral: 'border-line bg-elevated text-muted',
  accent: 'border-accent/30 bg-accent/10 text-accent',
  ok: 'border-ok/30 bg-ok/10 text-ok',
  warn: 'border-warn/30 bg-warn/10 text-warn',
  danger: 'border-danger/30 bg-danger/10 text-danger',
};

export interface BadgeProps {
  tone?: Tone;
  className?: string;
  title?: string;
  children: ReactNode;
}

export function Badge({ tone = 'neutral', className, title, children }: BadgeProps) {
  return (
    <span
      title={title}
      className={cn(
        'inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-[11px] font-medium leading-4',
        TONES[tone],
        className,
      )}
    >
      {children}
    </span>
  );
}

/** 状态徽章：受控词表 → 中文标签 + 语义色（未知值原样展示）。 */
export function StatusBadge({ status, className }: { status: string; className?: string }) {
  return (
    <Badge tone={statusTone(status)} className={className} title={status}>
      {statusLabel(status)}
    </Badge>
  );
}
