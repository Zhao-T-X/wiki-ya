import { Link } from 'react-router-dom';

import { Badge } from '@/components/ui/Badge';
import { cn } from '@/lib/cn';
import { sourceTarget } from '@/lib/links';
import { searchKindLabel } from '@/lib/status';
import type { AskSource } from '@/types/ipc';

export interface SourceCardProps {
  source: AskSource;
  /** 正文里点了 [n] 角标时高亮对应卡片。 */
  highlighted?: boolean;
  className?: string;
}

/**
 * 答案引用的来源卡片。
 *
 * 此前这个组件有三份手写实现（SearchPage 内联、AskPage 的 sources 列表、
 * ClaimTraceCard 的证据链），且 SearchPage 那份漏了段号标注 —— 而
 * types/ipc.ts 的 AskSource 明确要求 part 非空时 UI 必须标注段号，否则用户
 * 看到的引文只来自长 chunk 的前半段，却以为看到了整块。
 */
export function SourceCard({ source, highlighted, className }: SourceCardProps) {
  const path = sourceTarget(source);

  return (
    <div
      id={`source-${source.index}`}
      className={cn(
        'rounded-lg border bg-canvas px-3 py-2 transition-colors',
        highlighted ? 'border-accent/60 bg-accent/5' : 'border-line',
        className,
      )}
    >
      <div className="flex flex-wrap items-center gap-1.5">
        <span className="font-mono text-[10px] text-muted">[{source.index}]</span>
        <Badge tone="neutral">{searchKindLabel(source.kind)}</Badge>
        {source.part !== undefined && source.part > 0 ? (
          <Badge tone="warn" title="引文只来自该长切片的第 N 段，不是整块">
            第 {source.part} 段
          </Badge>
        ) : null}
        {path ? (
          <Link to={path} className="truncate text-xs text-accent hover:underline">
            {source.title}
          </Link>
        ) : (
          <span className="truncate text-xs text-ink">{source.title}</span>
        )}
      </div>
      <p className="mt-1 whitespace-pre-wrap break-words text-meta leading-relaxed text-muted">
        {source.snippet}
      </p>
    </div>
  );
}

export interface SourceListProps {
  sources: AskSource[];
  /** 当前被正文角标点亮的引用序号。 */
  activeIndex?: number | null;
  className?: string;
}

/** 来源列表（答案下方的证据区）。 */
export function SourceList({ sources, activeIndex, className }: SourceListProps) {
  if (sources.length === 0) return null;

  return (
    <div className={cn('space-y-2', className)}>
      {sources.map((source) => (
        <SourceCard
          key={`${source.kind}-${source.id}-${source.index}`}
          source={source}
          highlighted={activeIndex === source.index}
        />
      ))}
    </div>
  );
}
