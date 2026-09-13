import type { ReactNode } from 'react';

import { Badge, StatusBadge } from '@/components/ui/Badge';
import { Card } from '@/components/ui/Card';
import { cn } from '@/lib/cn';
import { formatConfidence, formatDate, formatRelativeTime, humanizePredicate } from '@/lib/format';
import { claimTypeLabel, modalityLabel, polarityLabel } from '@/lib/status';
import type { ClaimCard as ClaimCardModel } from '@/types/ipc';

/**
 * 契约 `ClaimCard` 未定义 `validFrom` / `validUntil`（时间维度是 ➕ 新增能力，
 * 见 D2）。这里做**只读的防御性读取**：后端一旦补充字段即自动展示，
 * 否则不渲染——不修改契约类型、不凭空发明字段。
 */
type ClaimWithValidity = ClaimCardModel & {
  validFrom?: string | null;
  validUntil?: string | null;
};

/** 把结构化 Claim 还原成可读句子。 */
export function claimSentence(claim: ClaimCardModel): string {
  const object = claim.objectName ?? claim.objectText ?? claim.content ?? '（未结构化对象）';
  const verb = `${claim.polarity === 'negative' ? 'not ' : ''}${humanizePredicate(claim.predicate)}`;
  return `${claim.subjectName} ${verb} ${object}`;
}

export interface ClaimCardProps {
  claim: ClaimCardModel;
  expanded?: boolean;
  onToggle?: (id: string) => void;
  /** 展开后渲染的内容（证据列表等）。 */
  children?: ReactNode;
}

export function ClaimCard({ claim, expanded = false, onToggle, children }: ClaimCardProps) {
  const validity = claim as ClaimWithValidity;
  const hasValidity = Boolean(validity.validFrom || validity.validUntil);

  return (
    <Card className={cn('p-3.5 transition-colors', expanded && 'border-accent/30')}>
      <button
        type="button"
        disabled={!onToggle}
        onClick={() => onToggle?.(claim.id)}
        className="w-full text-left disabled:cursor-default"
      >
        <div className="flex items-start justify-between gap-3">
          <p className="text-sm leading-snug text-ink">{claimSentence(claim)}</p>
          <div className="flex shrink-0 items-center gap-1.5">
            {claim.lifecycle === 'superseded' ? <Badge>历史</Badge> : null}
            <StatusBadge status={claim.status} />
          </div>
        </div>

        <div className="mt-2 flex flex-wrap items-center gap-x-2.5 gap-y-1 text-[11px] text-muted">
          <span>置信度 {formatConfidence(claim.confidence)}</span>
          <span className="text-line">·</span>
          <span>{claimTypeLabel(claim.claimType)}</span>
          <span className="text-line">·</span>
          <span>
            {polarityLabel(claim.polarity)} / {modalityLabel(claim.modality)}
          </span>
          {hasValidity ? (
            <>
              <span className="text-line">·</span>
              <span>
                有效 {formatDate(validity.validFrom)} → {validity.validUntil ? formatDate(validity.validUntil) : '至今'}
              </span>
            </>
          ) : null}
        </div>

        <div className="mt-1.5 flex flex-wrap items-center gap-x-2.5 gap-y-1 text-[11px] text-muted/80">
          <span className="truncate">来源：{claim.sourceDocumentTitle ?? '未关联文档'}</span>
          <span className="text-line">·</span>
          <span>{formatRelativeTime(claim.createdAt)}</span>
          {onToggle ? (
            <span className="ml-auto text-accent/80">{expanded ? '收起证据' : '查看证据'}</span>
          ) : null}
        </div>

        {claim.sourceQuote ? (
          <blockquote className="mt-2 border-l-2 border-line pl-3 text-[11px] italic leading-relaxed text-muted">
            {claim.sourceQuote}
          </blockquote>
        ) : null}
      </button>

      {expanded ? <div className="mt-3 border-t border-line pt-3">{children}</div> : null}
    </Card>
  );
}
