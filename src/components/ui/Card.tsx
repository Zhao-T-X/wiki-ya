import { cva, type VariantProps } from 'class-variance-authority';
import type { ReactNode } from 'react';

import { cn } from '@/lib/cn';

/**
 * 容器原语。
 *
 * PR-07 起引入两个新变体，用来解决「满屏 border + card + badge」的后台观感：
 *
 * - `quiet`：无边框，靠留白与背景层次区分区块。**这是内容区应该用的**——
 *   知识正文不需要再套一层描边。
 * - `flush`：完全无样式容器，仅提供语义与间距。
 *
 * `surface`（默认）保持改造前的类名逐字不变，因此现有 200+ 处调用点零视觉回归。
 */
const cardVariants = cva('rounded-xl', {
  variants: {
    tone: {
      surface: 'border border-line bg-surface',
      elevated: 'border border-line bg-elevated',
      quiet: 'bg-elevated/40',
      flush: '',
    },
  },
  defaultVariants: { tone: 'surface' },
});

export type CardTone = NonNullable<VariantProps<typeof cardVariants>['tone']>;

export interface CardProps extends VariantProps<typeof cardVariants> {
  className?: string;
  children: ReactNode;
}

export function Card({ tone, className, children }: CardProps) {
  return <div className={cn(cardVariants({ tone }), className)}>{children}</div>;
}

/** 卡片标题区：18px/600 起步，替代此前满屏的 `text-[11px] font-semibold uppercase`。 */
export function CardHeader({
  title,
  hint,
  action,
  className,
}: {
  title: ReactNode;
  hint?: ReactNode;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn('flex items-start justify-between gap-3 px-5 pt-4', className)}>
      <div className="min-w-0">
        <h3 className="text-sm font-semibold leading-snug text-ink">{title}</h3>
        {hint ? <p className="mt-0.5 text-xs leading-relaxed text-muted">{hint}</p> : null}
      </div>
      {action ? <div className="shrink-0">{action}</div> : null}
    </div>
  );
}

/** 卡片内容区：默认 20px 内边距，正好是推荐的正文行宽起点。 */
export function CardBody({ className, children }: { className?: string; children: ReactNode }) {
  return <div className={cn('p-5', className)}>{children}</div>;
}

/** 卡片底部：默认上边框分隔，用于放置操作区。 */
export function CardFooter({ className, children }: { className?: string; children: ReactNode }) {
  return (
    <div className={cn('border-t border-line px-5 py-3', className)}>{children}</div>
  );
}

export { cardVariants };
