import { useMemo, useState } from 'react';
import { Link } from 'react-router-dom';

import { ErrorNotice } from '@/components/ErrorNotice';
import { PageHeader } from '@/components/PageHeader';
import { ReviewIcon } from '@/components/icons';
import { Badge, StatusBadge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { EmptyState } from '@/components/ui/EmptyState';
import { Spinner } from '@/components/ui/Spinner';
import { decide_claim_relation, list_review_items, WikiError } from '@/lib/api';
import { cn } from '@/lib/cn';
import { formatConfidence } from '@/lib/format';
import { useAsyncData } from '@/lib/hooks';
import { relationshipLabel, relationshipTone } from '@/lib/status';
import { useUiStore } from '@/stores/ui';
import type { ClaimRelationDecision, ReviewItem } from '@/types/ipc';

/** 契约映射：Accept→accept，Keep Both→reset，Reject→reject。 */
const DECISIONS: { label: string; decision: ClaimRelationDecision; variant: 'primary' | 'secondary' | 'danger'; hint: string }[] = [
  { label: 'Accept', decision: 'accept', variant: 'primary', hint: '接受建议的关系（supersedes 会让旧 Claim 进入 superseded）' },
  { label: 'Keep Both', decision: 'reset', variant: 'secondary', hint: '回退该关系，保留两者（精确恢复 targetPreviousStatus）' },
  { label: 'Reject', decision: 'reject', variant: 'danger', hint: '拒绝该关系，不改动任何 Claim' },
];

export function ReviewPage() {
  const reviewFilter = useUiStore((state) => state.reviewFilter);
  const setReviewFilter = useUiStore((state) => state.setReviewFilter);

  const items = useAsyncData(() => list_review_items({ limit: 100 }), []);

  const [busyId, setBusyId] = useState<string | null>(null);
  const [actionError, setActionError] = useState<WikiError | null>(null);

  const all = items.data ?? [];
  const relationshipTypes = useMemo(
    () => [...new Set(all.map((item) => item.relation.relationship))],
    [all],
  );

  const visible = useMemo(() => {
    const filtered = reviewFilter ? all.filter((item) => item.relation.relationship === reviewFilter) : all;
    return [...filtered].sort(
      (a, b) => a.priority - b.priority || (b.relation.confidence ?? 0) - (a.relation.confidence ?? 0),
    );
  }, [all, reviewFilter]);

  async function decide(item: ReviewItem, decision: ClaimRelationDecision) {
    setBusyId(item.relation.id);
    setActionError(null);
    try {
      await decide_claim_relation({ relationId: item.relation.id, decision });
      items.reload();
    } catch (cause: unknown) {
      setActionError(cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause)));
    } finally {
      setBusyId(null);
    }
  }

  return (
    <div>
      <PageHeader
        title="Review"
        subtitle="每一条知识变更都必须回答：改了什么 / 为什么 / 证据 / 影响。这里是你唯一能改变 Claim 状态的入口。"
      />

      {relationshipTypes.length > 0 ? (
        <div className="mb-4 flex flex-wrap gap-2">
          <button
            type="button"
            onClick={() => setReviewFilter(null)}
            className={cn(
              'rounded-md border px-2.5 py-1 text-[11px] font-medium transition-colors',
              reviewFilter === null ? 'border-accent/40 bg-accent/10 text-accent' : 'border-line bg-elevated text-muted hover:text-ink',
            )}
          >
            全部
          </button>
          {relationshipTypes.map((relationship) => (
            <button
              key={relationship}
              type="button"
              onClick={() => setReviewFilter(relationship)}
              className={cn(
                'rounded-md border px-2.5 py-1 text-[11px] font-medium transition-colors',
                reviewFilter === relationship
                  ? 'border-accent/40 bg-accent/10 text-accent'
                  : 'border-line bg-elevated text-muted hover:text-ink',
              )}
            >
              {relationshipLabel(relationship)}
            </button>
          ))}
        </div>
      ) : null}

      {items.error ? <ErrorNotice error={items.error} /> : null}
      {actionError ? <ErrorNotice error={actionError} className="mb-3" /> : null}

      {items.loading && !items.data ? (
        <div className="flex items-center gap-2 py-6 text-xs text-muted">
          <Spinner className="h-3.5 w-3.5" />
          加载待审队列…
        </div>
      ) : null}

      {!items.loading && visible.length === 0 && !items.error ? (
        <EmptyState
          title="当前没有需要你决策的知识变更"
          description="当捕获的新内容与已有知识产生重复、补充或冲突时，系统会把它们放到这里，并解释原因与影响。"
          icon={<ReviewIcon className="h-5 w-5" />}
        />
      ) : null}

      <div className="space-y-3">
        {visible.map((item) => {
          const { relation } = item;
          const busy = busyId === relation.id;

          return (
            <Card key={relation.id} className="p-4">
              <div className="flex flex-wrap items-center gap-2">
                <Badge tone={relationshipTone(relation.relationship)}>
                  {relationshipLabel(relation.relationship)}
                </Badge>
                <StatusBadge status={relation.status} />
                <span className="text-[11px] text-muted">优先级 {item.priority}</span>
                <span className="text-line">·</span>
                <span className="text-[11px] text-muted">置信度 {formatConfidence(relation.confidence)}</span>
                {relation.suggestedAction ? (
                  <span className="ml-auto text-[11px] text-muted">建议动作：{relation.suggestedAction}</span>
                ) : null}
              </div>

              <div className="mt-3 grid gap-3 sm:grid-cols-2">
                <div className="rounded-lg border border-line bg-canvas p-3">
                  <p className="text-[10px] font-semibold uppercase tracking-wider text-accent">New（发起方）</p>
                  <p className="mt-1 text-xs leading-relaxed text-ink/90">{relation.sourceText}</p>
                  <Link
                    to={`/claims/${relation.sourceClaimId}`}
                    className="mt-1.5 inline-block text-[10px] text-accent hover:underline"
                  >
                    查看该 Claim
                  </Link>
                </div>
                <div className="rounded-lg border border-line bg-canvas p-3">
                  <p className="text-[10px] font-semibold uppercase tracking-wider text-muted">Existing（受影响方）</p>
                  <p className="mt-1 text-xs leading-relaxed text-ink/90">{relation.targetText}</p>
                  <Link
                    to={`/claims/${relation.targetClaimId}`}
                    className="mt-1.5 inline-block text-[10px] text-accent hover:underline"
                  >
                    查看该 Claim
                  </Link>
                </div>
              </div>

              <dl className="mt-3 space-y-2 text-xs">
                <div>
                  <dt className="text-[10px] font-semibold uppercase tracking-wider text-muted">改了什么</dt>
                  <dd className="mt-0.5 leading-relaxed text-ink/90">{item.whatChanged}</dd>
                </div>
                <div>
                  <dt className="text-[10px] font-semibold uppercase tracking-wider text-muted">为什么</dt>
                  <dd className="mt-0.5 leading-relaxed text-ink/90">{item.why}</dd>
                </div>
                <div>
                  <dt className="text-[10px] font-semibold uppercase tracking-wider text-muted">证据</dt>
                  <dd className="mt-0.5">
                    {item.evidenceQuote ? (
                      <blockquote className="border-l-2 border-accent/40 pl-3 italic leading-relaxed text-muted">
                        {item.evidenceQuote}
                      </blockquote>
                    ) : (
                      <span className="text-muted">该判定未附带引文（确定性规则推断）。</span>
                    )}
                  </dd>
                </div>
                <div>
                  <dt className="text-[10px] font-semibold uppercase tracking-wider text-muted">影响</dt>
                  <dd className="mt-0.5 leading-relaxed text-ink/90">{item.impact}</dd>
                </div>
              </dl>

              {relation.reason ? (
                <p className="mt-2 text-[11px] text-muted/80">规则说明：{relation.reason}</p>
              ) : null}

              <div className="mt-3 flex flex-wrap gap-2 border-t border-line pt-3">
                {DECISIONS.map((option) => (
                  <Button
                    key={option.decision}
                    size="sm"
                    variant={option.variant}
                    title={option.hint}
                    loading={busy}
                    disabled={busy}
                    onClick={() => decide(item, option.decision)}
                  >
                    {option.label}
                  </Button>
                ))}
              </div>
            </Card>
          );
        })}
      </div>
    </div>
  );
}
