import { useMemo, useState } from 'react';

import { ReviewDecisionPanel } from '@/features/review/ReviewDecisionPanel';
import { ReviewQueue } from '@/features/review/ReviewQueue';
import { ErrorNotice } from '@/components/ErrorNotice';
import { PageHeader } from '@/components/PageHeader';
import { Card } from '@/components/ui/Card';
import { EmptyState } from '@/components/ui/EmptyState';
import { ReviewIcon } from '@/components/icons';
import { decide_claim_relation, list_review_items, WikiError } from '@/lib/api';
import { useAsyncData } from '@/lib/hooks';
import { relationshipLabel } from '@/lib/status';
import { useUiStore } from '@/stores/ui';
import type { ClaimRelationDecision, ReviewItem } from '@/types/ipc';

/**
 * 队列上限（任务书 §24）。
 *
 * 此前是 `limit: 100` 且**一次性渲染 100 张完整详情卡**。现在：拉 40 条，
 * 但**只渲染当前选中项的详情**，队列项本身是轻量的两行摘要。
 */
const QUEUE_LIMIT = 40;

const DECISION_LABELS: Record<ClaimRelationDecision, string> = {
  accept: '接受建议',
  reset: '保留两者',
  reject: '拒绝',
};

/**
 * Review = 决策工作台（任务书 §18）。
 *
 * 布局：左队列 / 右决策。右侧四段固定，底部操作区 sticky。
 *
 * 交互（任务书 §23，核心改动）：
 * 决策成功后**不整表 reload**，而是用 `useAsyncData.setData` 把这一条从队列里
 * 局部移除；「当前项」是**派生**出来的（selectedId 找不到就落到第一条），
 * 因此移除后自动选中下一条，无需额外同步逻辑。失败时条目保留并显示错误。
 */
export function ReviewPage() {
  const reviewFilter = useUiStore((state) => state.reviewFilter);
  const setReviewFilter = useUiStore((state) => state.setReviewFilter);

  const items = useAsyncData(() => list_review_items({ limit: QUEUE_LIMIT }), []);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [actionError, setActionError] = useState<WikiError | null>(null);
  const [lastDone, setLastDone] = useState<string | null>(null);

  const visible = useMemo(() => {
    const all = items.data ?? [];
    const filtered = reviewFilter
      ? all.filter((item) => item.relation.relationship === reviewFilter)
      : all;
    return [...filtered].sort(
      (a, b) => a.priority - b.priority || (b.relation.confidence ?? 0) - (a.relation.confidence ?? 0),
    );
  }, [items.data, reviewFilter]);

  // 派生当前项：显式选择失效（已处理 / 被筛掉）时自动落到第一条。
  const current: ReviewItem | null =
    visible.find((item) => item.relation.id === selectedId) ?? visible[0] ?? null;

  async function decide(item: ReviewItem, decision: ClaimRelationDecision) {
    setBusyId(item.relation.id);
    setActionError(null);
    setLastDone(null);
    try {
      await decide_claim_relation({ relationId: item.relation.id, decision });

      // 局部更新：只移除这一条，不重新拉整表（任务书 §23）。
      items.setData((prev) => prev?.filter((row) => row.relation.id !== item.relation.id) ?? prev);
      setLastDone(
        `${relationshipLabel(item.relation.relationship)}：已按「${DECISION_LABELS[decision]}」处理。`,
      );
      // 清空显式选择 → current 自动落到下一条。
      setSelectedId(null);
    } catch (cause: unknown) {
      // 失败时**保留条目**（用户可以重试或换动作），只报错。
      setActionError(cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause)));
    } finally {
      setBusyId(null);
    }
  }

  const busy = current !== null && busyId === current.relation.id;

  return (
    <div className="mx-auto w-full max-w-[1400px]">
      <PageHeader
        title="Review"
        subtitle="逐条决定新知识与已有知识的关系。左边选一条，右边看依据，底部给动作。"
      />

      {items.error ? <ErrorNotice error={items.error} /> : null}
      {actionError ? <ErrorNotice error={actionError} className="mb-3" /> : null}
      {lastDone ? (
        <p className="mb-3 animate-fade-in rounded-lg border border-ok/30 bg-ok/10 px-3 py-2 text-xs text-ok">
          {lastDone}
        </p>
      ) : null}

      <div className="grid gap-4 lg:grid-cols-[minmax(0,360px)_minmax(0,1fr)]">
        <Card tone="quiet" className="flex max-h-[calc(100vh-14rem)] flex-col p-3">
          <ReviewQueue
            items={visible}
            loading={items.loading}
            filter={reviewFilter}
            onFilterChange={setReviewFilter}
            selectedId={current?.relation.id ?? null}
            onSelect={setSelectedId}
          />
        </Card>

        <Card tone="quiet" className="max-h-[calc(100vh-14rem)] p-5">
          {current ? (
            <ReviewDecisionPanel
              item={current}
              busy={busy}
              onDecide={(decision) => decide(current, decision)}
            />
          ) : (
            <EmptyState
              title="当前没有需要你决策的知识变更"
              description="当捕获的新内容与已有知识产生重复、补充或冲突时，系统会把它们放到这里，并解释原因与影响。"
              icon={<ReviewIcon className="h-5 w-5" />}
            />
          )}
        </Card>
      </div>
    </div>
  );
}
