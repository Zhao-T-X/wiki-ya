import { Link } from 'react-router-dom';

import { MarkdownContent } from '@/components/content/MarkdownContent';
import { Badge } from '@/components/ui/Badge';
import { cn } from '@/lib/cn';
import { sourceTarget } from '@/lib/links';
import type { AskSource } from '@/types/ipc';

/**
 * 证据块（任务书 §11）。
 *
 * 此前证据有三种各自为政的呈现：`EvidenceList` 的 blockquote、AskPage 的
 * sources 列表、ClaimTraceCard 的 `<pre>` 原文。它们的共同缺陷是**只显示引文，
 * 不显示"这段话凭什么可信"**——而 provenance 恰恰是本项目的核心价值。
 *
 * 这里把四件事放在一起：引文 / 来源 / 定位（chunk + 段号）/ 支持度。
 */

export type SupportLevel = 'directly' | 'partially' | 'unsupported';

/** 支持度 → 中文标签 + 语义色。与后端 `SupportLevel` 一一对应。 */
const SUPPORT: Record<SupportLevel, { label: string; tone: 'ok' | 'warn' | 'danger' }> = {
  directly: { label: '直接引文', tone: 'ok' },
  partially: { label: '部分对应', tone: 'warn' },
  unsupported: { label: '未找到引文', tone: 'danger' },
};

export interface EvidenceBlockProps {
  /** 引文正文（Markdown）。 */
  quote: string;
  /** 来源标题（文档 / Claim / Entity）。 */
  sourceTitle?: string;
  /** 来源 kind，用于生成跳转链接。 */
  sourceKind?: string;
  sourceId?: string;
  /** 来自哪个切片（用于回溯原文）。 */
  chunkIndex?: number;
  /** 引文只来自长切片的第几段（1-based；undefined = 整块）。 */
  part?: number;
  supportLevel?: SupportLevel;
  /** 溯源入口（如 Claim 详情页）。 */
  tracePath?: string;
  className?: string;
}

export function EvidenceBlock({
  quote,
  sourceTitle,
  sourceKind,
  sourceId,
  chunkIndex,
  part,
  supportLevel,
  tracePath,
  className,
}: EvidenceBlockProps) {
  const path = sourceKind && sourceId ? sourceTarget({ kind: sourceKind, id: sourceId } as AskSource) : null;
  const support = supportLevel ? SUPPORT[supportLevel] : null;

  return (
    <figure
      className={cn(
        'group rounded-xl border border-line bg-canvas px-4 py-3 transition-colors hover:border-line/80',
        className,
      )}
    >
      {/* 引文：走 MarkdownContent compact，这样引文里的 `code` 与强调也是渲染过的 */}
      <blockquote className="border-l-2 border-accent/40 pl-3 text-sm leading-relaxed text-ink/85">
        <MarkdownContent mode="compact">{quote}</MarkdownContent>
      </blockquote>

      <figcaption className="mt-2.5 flex flex-wrap items-center gap-x-2 gap-y-1 pl-3 text-meta text-muted">
        {sourceTitle ? (
          path ? (
            <Link to={path} className="truncate text-xs text-accent hover:underline">
              {sourceTitle}
            </Link>
          ) : (
            <span className="truncate text-xs text-ink">{sourceTitle}</span>
          )
        ) : null}

        {chunkIndex !== undefined ? (
          <span className="font-mono text-[10px]">Chunk #{chunkIndex}</span>
        ) : null}

        {part !== undefined && part > 0 ? (
          <Badge tone="warn" title="引文只来自该长切片的这一段，不是整块">
            第 {part} 段
          </Badge>
        ) : null}

        {support ? (
          <Badge tone={support.tone} title="这段引文与结论的对应强度">
            {support.label}
          </Badge>
        ) : null}

        {tracePath ? (
          <Link
            to={tracePath}
            className="ml-auto shrink-0 text-meta text-muted opacity-0 transition-opacity hover:text-accent group-hover:opacity-100"
          >
            查看溯源 →
          </Link>
        ) : null}
      </figcaption>
    </figure>
  );
}
