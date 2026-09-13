import { useState } from 'react';

import { EvidenceList } from '@/components/EvidenceList';
import { ErrorNotice } from '@/components/ErrorNotice';
import { ClaimCard as ClaimCardView } from '@/components/ClaimCard';
import { Badge } from '@/components/ui/Badge';
import { Spinner } from '@/components/ui/Spinner';
import { get_claim } from '@/lib/api';
import { useAsyncData } from '@/lib/hooks';
import { relationshipLabel, relationshipTone } from '@/lib/status';
import type { ClaimCard } from '@/types/ipc';

export interface ClaimListProps {
  claims: ClaimCard[];
  emptyText?: string;
}

/**
 * Claim 列表 + 按需展开证据。
 * 证据来自 `get_claim`（契约中 ClaimCard 无 evidenceCount 字段，故展开时读取详情）。
 */
export function ClaimList({ claims, emptyText }: ClaimListProps) {
  const [expandedId, setExpandedId] = useState<string | null>(null);

  const detail = useAsyncData(
    () => (expandedId ? get_claim({ id: expandedId }) : Promise.resolve(null)),
    [expandedId],
    expandedId !== null,
  );

  if (claims.length === 0) {
    return <p className="text-xs text-muted">{emptyText ?? '暂无 Claim。'}</p>;
  }

  return (
    <div className="space-y-2.5">
      {claims.map((claim) => {
        const expanded = expandedId === claim.id;
        const current = detail.data && detail.data.claim.id === claim.id ? detail.data : null;
        const relations = current?.relations ?? [];

        return (
          <ClaimCardView
            key={claim.id}
            claim={claim}
            expanded={expanded}
            onToggle={(id) => setExpandedId((prev) => (prev === id ? null : id))}
          >
            {detail.loading && !current ? (
              <div className="flex items-center gap-2 text-xs text-muted">
                <Spinner className="h-3.5 w-3.5" />
                正在读取证据…
              </div>
            ) : null}

            {detail.error ? <ErrorNotice error={detail.error} /> : null}

            {current ? <EvidenceList evidence={current.evidence} /> : null}

            {relations.length > 0 ? (
              <div className="mt-3">
                <p className="mb-1.5 text-[11px] font-medium uppercase tracking-wide text-muted">演化关系</p>
                <ul className="space-y-1.5">
                  {relations.map((relation) => (
                    <li key={relation.id} className="text-[11px] leading-relaxed text-muted">
                      <Badge tone={relationshipTone(relation.relationship)}>
                        {relationshipLabel(relation.relationship)}
                      </Badge>
                      <span className="ml-2">
                        {relation.sourceText} → {relation.targetText}
                      </span>
                    </li>
                  ))}
                </ul>
              </div>
            ) : null}
          </ClaimCardView>
        );
      })}
    </div>
  );
}
