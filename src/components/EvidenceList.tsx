import { Link } from 'react-router-dom';

import { MarkdownContent } from '@/components/content/MarkdownContent';
import { Badge } from '@/components/ui/Badge';
import { cn } from '@/lib/cn';
import type { EvidenceCard } from '@/types/ipc';

export interface EvidenceListProps {
  evidence: EvidenceCard[];
  emptyText?: string;
  /** 是否显示「查看溯源」入口（Claim 详情页里自己是当前页，不需要自跳）。 */
  showTrace?: boolean;
}

/**
 * 证据列表（Claim 视角）。
 *
 * 与 `content/EvidenceBlock` 的分工——两者语义**不同**，不能合并：
 *
 * - `EvidenceBlock`：答案里的引用，带 `supportLevel`（PR-03 的引文对应强度：
 *   directly / partially / unsupported）。
 * - `EvidenceList`：Claim 的证据链，带 `evidenceLevel`（L1–L5 的**来源层级**：
 *   人工录入 / 摘录 / 段落 / 切片 / 多切片）。L1-L5 与 supportLevel 是两套正交的
 *   维度，把 L3 显示成「部分对应」是错的信息。
 *
 * PR-07 §11 的要求在这里落地：引文不再只是 blockquote，而是走统一内容渲染
 * （引文里也可能有 `code` 与强调），并补上此前缺失的原文跳转。
 */
export function EvidenceList({ evidence, emptyText = '暂无证据引用。', showTrace }: EvidenceListProps) {
  if (evidence.length === 0) {
    return <p className="text-secondary text-muted">{emptyText}</p>;
  }

  return (
    <ul className="space-y-2">
      {evidence.map((item) => (
        <li key={item.id} className="rounded-lg border border-line bg-canvas p-3">
          <div className="flex items-center justify-between gap-2">
            <Badge tone="accent" title={`evidenceLevel=${item.evidenceLevel}`}>
              L{item.evidenceLevel} · {item.evidenceLevelName}
            </Badge>
            <span className="font-mono text-[10px] text-muted">
              {item.startOffset != null && item.endOffset != null
                ? `${item.startOffset}–${item.endOffset}`
                : '—'}
            </span>
          </div>

          {item.quote ? (
            <blockquote className="mt-2 border-l-2 border-accent/40 pl-3">
              <MarkdownContent mode="compact" className="text-secondary italic">
                {item.quote}
              </MarkdownContent>
            </blockquote>
          ) : (
            <p className="mt-2 text-xs text-muted">该证据未存储引文，需回原文查看。</p>
          )}

          <p className="mt-2 flex flex-wrap items-center gap-x-2 gap-y-1 text-[11px] text-muted">
            <span>来源文档</span>
            {item.documentId ? (
              <Link
                to={`/documents/${item.documentId}`}
                className={cn('truncate text-xs text-accent hover:underline')}
              >
                {item.documentTitle ?? '（标题不可用）'}
              </Link>
            ) : (
              <span className="text-ink/80">{item.documentTitle ?? '未知（来源已不可用）'}</span>
            )}
            {showTrace ? (
              <Link to={`/claims/${item.id}`} className="ml-auto shrink-0 hover:text-accent">
                查看溯源 →
              </Link>
            ) : null}
          </p>
        </li>
      ))}
    </ul>
  );
}
