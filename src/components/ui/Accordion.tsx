import * as AccordionPrimitive from '@radix-ui/react-accordion';
import type { ComponentPropsWithoutRef } from 'react';

import { ChevronRightIcon } from '@/components/icons';
import { cn } from '@/lib/cn';

/**
 * 折叠面板（Radix）。
 *
 * 与上一轮建的 `agent/Collapse.tsx` 的分工：
 *
 * - `Collapse`：**单个**折叠块，自带标题按钮（我们自绘，样式更自由）；
 * - `Accordion`：**一组**互斥或并列的折叠块，需要正确的 `aria-expanded` /
 *   键盘方向键 / 单开互斥。这是「为什么这样判断 ▾」这类可折叠说明区的正确原语。
 */
export const Accordion = AccordionPrimitive.Root;

export function AccordionItem({
  className,
  ...props
}: ComponentPropsWithoutRef<typeof AccordionPrimitive.Item>) {
  return <AccordionPrimitive.Item className={cn('border-b border-line last:border-b-0', className)} {...props} />;
}

export function AccordionTrigger({
  className,
  children,
  ...props
}: ComponentPropsWithoutRef<typeof AccordionPrimitive.Trigger>) {
  return (
    <AccordionPrimitive.Header className="flex">
      <AccordionPrimitive.Trigger
        className={cn(
          'group flex flex-1 items-center gap-2 py-2.5 text-left text-xs font-medium text-muted transition-colors',
          'hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50',
          className,
        )}
        {...props}
      >
        <ChevronRightIcon className="h-3.5 w-3.5 shrink-0 transition-transform group-data-[state=open]:rotate-90" />
        {children}
      </AccordionPrimitive.Trigger>
    </AccordionPrimitive.Header>
  );
}

export function AccordionContent({
  className,
  children,
  ...props
}: ComponentPropsWithoutRef<typeof AccordionPrimitive.Content>) {
  return (
    <AccordionPrimitive.Content
      className={cn(
        'overflow-hidden text-xs leading-relaxed text-muted',
        'data-[state=open]:animate-accordion-down data-[state=closed]:animate-accordion-up',
        className,
      )}
      {...props}
    >
      <div className="pb-3 pl-[1.375rem]">{children}</div>
    </AccordionPrimitive.Content>
  );
}
