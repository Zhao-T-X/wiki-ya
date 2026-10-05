import * as TooltipPrimitive from '@radix-ui/react-tooltip';
import type { ComponentPropsWithoutRef, ReactNode } from 'react';

import { cn } from '@/lib/cn';

/**
 * 工具提示（Radix）。
 *
 * 此前全项目只有原生 `title=` 属性充当 tooltip——它在触摸设备上不触发、
 * 无法控制样式、延迟不可控、键盘聚焦时也不出现。Radix 版补齐了这些。
 *
 * 注意：`Tooltip.Provider` 的 `delayDuration` 调短到 200ms：知识工作台里
 * 「为什么相关 ▾」这类提示是高频操作，默认 700ms 会让人觉得界面迟钝。
 */
export const TooltipProvider = ({ children }: { children: ReactNode }) => (
  <TooltipPrimitive.Provider delayDuration={200} skipDelayDuration={300}>
    {children}
  </TooltipPrimitive.Provider>
);

export const Tooltip = TooltipPrimitive.Root;
export const TooltipTrigger = TooltipPrimitive.Trigger;

export function TooltipContent({
  className,
  sideOffset = 6,
  ...props
}: ComponentPropsWithoutRef<typeof TooltipPrimitive.Content>) {
  return (
    <TooltipPrimitive.Portal>
      <TooltipPrimitive.Content
        sideOffset={sideOffset}
        className={cn(
          'z-50 max-w-xs rounded-lg border border-line bg-elevated px-2.5 py-1.5',
          'text-meta leading-relaxed text-ink shadow-xl',
          'data-[state=delayed-open]:animate-in data-[state=delayed-open]:fade-in-0 data-[state=delayed-open]:zoom-in-95',
          className,
        )}
        {...props}
      />
    </TooltipPrimitive.Portal>
  );
}

/** 薄封装：`<Tooltip label="…"><button/></Tooltip>`。 */
export function Hint({ label, children }: { label: ReactNode; children: ReactNode }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>{children}</TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}
