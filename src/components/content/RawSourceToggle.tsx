import { useState, type ReactNode } from 'react';

import { MarkdownContent } from '@/components/content/MarkdownContent';
import { cn } from '@/lib/cn';

/**
 * 渲染 / 原始 双视图（任务书 §10）。
 *
 * **默认渲染，但不是 Source 的替代品。** 原始视图保证"看到的字符就是数据库里
 * 的内容"——这是 provenance 的一部分：一个 Markdown 渲染器可能有 bug、可能被
 * 投毒（sanitize 会剥掉某些内容），此时用户必须有一条不经过任何解析的核对路径。
 */
export function RawSourceToggle({
  source,
  defaultView = 'rendered',
  className,
}: {
  source: string;
  defaultView?: 'rendered' | 'raw';
  className?: string;
}) {
  const [view, setView] = useState<'rendered' | 'raw'>(defaultView);

  return (
    <div className={cn('space-y-3', className)}>
      <div
        role="tablist"
        aria-label="原文视图切换"
        className="inline-flex items-center gap-1 rounded-lg border border-line bg-surface p-0.5"
      >
        {(
          [
            { key: 'rendered' as const, label: '渲染' },
            { key: 'raw' as const, label: '原始' },
          ]
        ).map((item) => (
          <button
            key={item.key}
            type="button"
            role="tab"
            aria-selected={view === item.key}
            onClick={() => setView(item.key)}
            className={cn(
              'rounded-md px-3 py-1 text-xs font-medium transition-colors',
              'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50',
              view === item.key ? 'bg-accent/10 text-accent' : 'text-muted hover:text-ink',
            )}
          >
            {item.label}
          </button>
        ))}
        {view === 'raw' ? (
          <span className="px-2 text-[10px] text-muted">所见即数据库内容</span>
        ) : null}
      </div>

      <div className={cn(view === 'raw' ? 'max-h-[560px] overflow-y-auto' : 'max-w-reading')}>
        <MarkdownContent mode={view === 'raw' ? 'source' : 'full'}>{source}</MarkdownContent>
      </div>
    </div>
  );
}

/** 供页面在标题区自定义右侧内容时使用。 */
export function RawSourceLabel({ children }: { children: ReactNode }) {
  return <span className="text-meta text-muted">{children}</span>;
}
