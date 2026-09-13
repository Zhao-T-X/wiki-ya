import { Badge } from '@/components/ui/Badge';
import type { EvidenceCard } from '@/types/ipc';

export interface EvidenceListProps {
  evidence: EvidenceCard[];
  emptyText?: string;
}

/** 证据列表：引文 + 证据层级名 + 所在文档 + 原文偏移。 */
export function EvidenceList({ evidence, emptyText = '暂无证据引用。' }: EvidenceListProps) {
  if (evidence.length === 0) {
    return <p className="text-xs text-muted">{emptyText}</p>;
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
            <blockquote className="mt-2 border-l-2 border-accent/40 pl-3 text-xs italic leading-relaxed text-ink/90">
              {item.quote}
            </blockquote>
          ) : (
            <p className="mt-2 text-xs text-muted">该证据未存储引文，需回原文查看。</p>
          )}

          <p className="mt-2 text-[11px] text-muted">
            来源文档：<span className="text-ink/80">{item.documentTitle ?? '未知（来源已不可用）'}</span>
          </p>
        </li>
      ))}
    </ul>
  );
}
