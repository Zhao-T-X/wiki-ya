import { cva, type VariantProps } from 'class-variance-authority';
import type { ReactNode } from 'react';

import { cn } from '@/lib/cn';
import { statusLabel, statusTone, type Tone } from '@/lib/status';

/**
 * 语义色徽章。
 *
 * 类名与改造前逐字一致（零视觉回归），只把色板搬进 cva，并新增 `solid` 变体。
 *
 * PR-07 的一条设计约束是「减少 badge 堆叠」——同一张卡片上超过 2 个 Badge 就该
 * 改成一行文字。`variant="text"` 是给这种场景准备的：保留语义色，去掉边框。
 */
const badgeVariants = cva(
  'inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-meta font-medium leading-4',
  {
    variants: {
      tone: {
        neutral: 'border-line bg-elevated text-muted',
        accent: 'border-accent/30 bg-accent/10 text-accent',
        ok: 'border-ok/30 bg-ok/10 text-ok',
        warn: 'border-warn/30 bg-warn/10 text-warn',
        danger: 'border-danger/30 bg-danger/10 text-danger',
      },
      variant: {
        bordered: '',
        text: 'border-transparent bg-transparent px-0',
        solid: 'border-transparent',
      },
    },
    defaultVariants: { tone: 'neutral', variant: 'bordered' },
  },
);

export interface BadgeProps extends VariantProps<typeof badgeVariants> {
  className?: string;
  title?: string;
  children: ReactNode;
}

export function Badge({ tone = 'neutral', variant, className, title, children }: BadgeProps) {
  return (
    <span title={title} className={cn(badgeVariants({ tone, variant }), className)}>
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

export type BadgeTone = Tone;
export { badgeVariants };
