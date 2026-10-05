import * as ScrollAreaPrimitive from '@radix-ui/react-scroll-area';
import type { ComponentPropsWithoutRef } from 'react';

import { cn } from '@/lib/cn';

/**
 * 自定义滚动区（Radix）。
 *
 * 此前长列表统一用 `max-h-[520px] overflow-y-auto`——在 macOS 上这会让**内侧**
 * 内容区覆盖外层页面滚动条（overlapping scrollbars），滚动嵌套很别扭；
 * 而 Tauri 的 WKWebView 上滚动条样式又是另一套。Radix 的 ScrollArea 给出
 * 独立滚动条，行为跨平台一致。
 */
export function ScrollArea({
  className,
  children,
  ...props
}: ComponentPropsWithoutRef<typeof ScrollAreaPrimitive.Root>) {
  return (
    <ScrollAreaPrimitive.Root className={cn('relative overflow-hidden', className)} {...props}>
      <ScrollAreaPrimitive.Viewport className="h-full w-full rounded-[inherit]">
        {children}
      </ScrollAreaPrimitive.Viewport>
      <ScrollBar />
      <ScrollAreaPrimitive.Corner />
    </ScrollAreaPrimitive.Root>
  );
}

export function ScrollBar({
  className,
  orientation = 'vertical',
  ...props
}: ComponentPropsWithoutRef<typeof ScrollAreaPrimitive.ScrollAreaScrollbar>) {
  return (
    <ScrollAreaPrimitive.ScrollAreaScrollbar
      orientation={orientation}
      className={cn(
        'flex touch-none select-none transition-colors',
        orientation === 'vertical' && 'h-full w-2 border-l border-l-transparent p-[1px]',
        orientation === 'horizontal' && 'h-2 flex-col border-t border-t-transparent p-[1px]',
        className,
      )}
      {...props}
    >
      <ScrollAreaPrimitive.ScrollAreaThumb className="relative flex-1 rounded-full bg-line" />
    </ScrollAreaPrimitive.ScrollAreaScrollbar>
  );
}
