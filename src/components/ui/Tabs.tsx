import * as TabsPrimitive from '@radix-ui/react-tabs';
import type { ComponentPropsWithoutRef, ReactNode } from 'react';

import { cn } from '@/lib/cn';

/**
 * 选项卡（Radix 无样式原语 + 本项目视觉）。
 *
 * 此前项目里 tabs 只有 SettingsPage 内联的一段 map 出来的 button 组：没有
 * `role="tablist"`、没有键盘方向键切换、没有 aria 属性。Radix 补齐了这些
 * 可访问性，同时保持视觉由我们决定。
 */
export const Tabs = TabsPrimitive.Root;

export function TabsList({ className, ...props }: ComponentPropsWithoutRef<typeof TabsPrimitive.List>) {
  return (
    <TabsPrimitive.List
      className={cn('inline-flex items-center gap-1 rounded-lg border border-line bg-surface p-0.5', className)}
      {...props}
    />
  );
}

export function TabsTrigger({
  className,
  children,
  ...props
}: ComponentPropsWithoutRef<typeof TabsPrimitive.Trigger>) {
  return (
    <TabsPrimitive.Trigger
      className={cn(
        'inline-flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-medium text-muted transition-colors',
        'hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50',
        'data-[state=active]:bg-accent/10 data-[state=active]:text-accent',
        className,
      )}
      {...props}
    >
      {children}
    </TabsPrimitive.Trigger>
  );
}

export function TabsContent({
  className,
  children,
  ...props
}: ComponentPropsWithoutRef<typeof TabsPrimitive.Content>) {
  return (
    <TabsPrimitive.Content
      className={cn('mt-3 focus-visible:outline-none', className)}
      {...props}
    >
      {children}
    </TabsPrimitive.Content>
  );
}

/** 标签页的页签 + 内容薄封装：最常见的「两三个视图切换」。 */
export function TabPanel({
  tabs,
  children,
}: {
  tabs: { value: string; label: ReactNode }[];
  children: ReactNode;
}) {
  const first = tabs[0]?.value ?? '';
  return (
    <Tabs defaultValue={first}>
      <TabsList>
        {tabs.map((tab) => (
          <TabsTrigger key={tab.value} value={tab.value}>
            {tab.label}
          </TabsTrigger>
        ))}
      </TabsList>
      {tabs.map((tab) => (
        <TabsContent key={tab.value} value={tab.value}>
          {children}
        </TabsContent>
      ))}
    </Tabs>
  );
}
