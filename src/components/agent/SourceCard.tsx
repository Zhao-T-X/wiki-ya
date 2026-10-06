import { useState } from 'react';
import { Link } from 'react-router-dom';

import { MarkdownContent } from '@/components/content/MarkdownContent';
import { Citation } from '@/components/content/Citation';
import { Badge } from '@/components/ui/Badge';
import { Dialog } from '@/components/ui/Dialog';
import {
  DialogDescription,
  DialogFooter,
  DialogTitle,
  DrawerContent,
} from '@/components/ui/Dialog';
import { cn } from '@/lib/cn';
import { sourceTarget } from '@/lib/links';
import { searchKindLabel } from '@/lib/status';
import type { AskSource } from '@/types/ipc';

export interface SourceCardProps {
  source: AskSource;
  /** 正文里点了 [n] 角标时高亮对应卡片。 */
  highlighted?: boolean;
  /** 点击「查看原文」时触发，由列表统一弹出预览抽屉。 */
  onPreview?: (source: AskSource) => void;
  className?: string;
}

/**
 * 答案引用的来源条目（任务书 PR-07.1 Content Presentation Fix）。
 *
 * 此前这个组件有三份手写实现，且**直接把 `source.snippet` 当纯文本铺出来**
 * （`whitespace-pre-wrap`）——导致同一段 Markdown 在 Answer 正文里渲染正常，
 * 到了来源区却原样显示 ```` ``` ```` 围栏和 `---` 分隔线（第一张截图问题）。
 *
 * 现在统一走 `MarkdownContent mode="compact"`：来源区与正文、证据共用一套渲染，
 * 且只作为「证据摘要」而非「整篇原文」。完整内容通过「查看原文」抽屉核对。
 */
export function SourceCard({ source, highlighted, onPreview, className }: SourceCardProps) {
  const path = sourceTarget(source);

  return (
    <div
      id={`source-${source.index}`}
      className={cn(
        'rounded-lg border px-3 py-2.5 transition-colors',
        highlighted ? 'border-accent/60 bg-accent/5' : 'border-line/70 bg-canvas/40',
        className,
      )}
    >
      <div className="flex flex-wrap items-center gap-1.5">
        <Citation index={source.index} />
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

      {/* 摘要：compact 模式按块边界截断，避免把整个 chunk 铺出来（任务书 T3）。 */}
      <MarkdownContent mode="compact" className="mt-1.5">
        {source.snippet}
      </MarkdownContent>

      <div className="mt-1.5">
        <button
          type="button"
          onClick={() => onPreview?.(source)}
          className="text-meta text-muted transition-colors hover:text-accent"
        >
          查看原文 →
        </button>
      </div>
    </div>
  );
}

export interface SourceListProps {
  sources: AskSource[];
  /** 当前被正文角标点亮的引用序号。 */
  activeIndex?: number | null;
  className?: string;
}

/**
 * 来源列表（答案下方的证据区）。
 *
 * 同时统一承载「查看原文」预览抽屉：快速核对走抽屉（full 内容 + 打开文档），
 * 深入阅读走标题链接跳文档详情（任务书 PR-07.1 T6）。
 */
export function SourceList({ sources, activeIndex, className }: SourceListProps) {
  const [preview, setPreview] = useState<AskSource | null>(null);
  if (sources.length === 0) return null;

  const previewPath = preview ? sourceTarget(preview) : null;

  return (
    <div className={cn('space-y-2', className)}>
      {sources.map((source) => (
        <SourceCard
          key={`${source.kind}-${source.id}-${source.index}`}
          source={source}
          highlighted={activeIndex === source.index}
          onPreview={setPreview}
        />
      ))}

      <Dialog open={preview !== null} onOpenChange={(open) => (open ? undefined : setPreview(null))}>
        <DrawerContent>
          <div className="flex items-start justify-between gap-4">
            <div className="min-w-0">
              <DialogTitle className="truncate">{preview?.title}</DialogTitle>
              <DialogDescription>
                {preview ? searchKindLabel(preview.kind) : ''}
                {preview && preview.part !== undefined && preview.part > 0 ? ` · 第 ${preview.part} 段` : ''}
              </DialogDescription>
            </div>
            {preview ? <Citation index={preview.index} /> : null}
          </div>

          <div className="mt-4 max-h-[70vh] overflow-y-auto">
            {preview ? <MarkdownContent mode="full">{preview.snippet}</MarkdownContent> : null}
          </div>

          <DialogFooter>
            {previewPath ? (
              <Link
                to={previewPath}
                onClick={() => setPreview(null)}
                className="text-sm text-accent hover:underline"
              >
                打开文档 ↗
              </Link>
            ) : null}
            <button
              type="button"
              onClick={() => setPreview(null)}
              className="rounded-md border border-line px-3 py-1.5 text-sm text-ink transition-colors hover:bg-elevated"
            >
              关闭
            </button>
          </DialogFooter>
        </DrawerContent>
      </Dialog>
    </div>
  );
}
