import { Link } from 'react-router-dom';

import { Card } from '@/components/ui/Card';
import type { ReviewItem } from '@/types/ipc';

/**
 * 影响段（任务书 §21.4）。
 *
 * 回答一个具体问题：**「接受以后会发生什么」**。
 *
 * `accept` 与 `reset`/`reject` 的后果完全不同，只给一段 impact 文本不够——用户
 * 点按钮前必须知道自己在改变什么。因此对 supersedes（取代）额外给出结构化的
 * 「谁变成历史 / 谁成为当前」，这是唯一会改动知识状态的关系。
 */
export function ReviewImpact({ item }: { item: ReviewItem }) {
  const { relation } = item;
  const supersedes = relation.relationship === 'supersedes';

  return (
    <div className="space-y-2">
      <Card tone="quiet" className="p-3">
        <p className="text-[13px] leading-relaxed text-ink/90">{item.impact}</p>
      </Card>

      {supersedes ? (
        <Card tone="quiet" className="space-y-1.5 p-3">
          <p className="text-[11px] uppercase tracking-wider text-muted">接受后</p>
          <p className="flex items-start gap-2 text-[13px] leading-relaxed">
            <span className="mt-1.5 h-1.5 w-1.5 shrink-0 rounded-full bg-muted" />
            <span className="text-muted">
              <Link to={`/claims/${relation.targetClaimId}`} className="hover:text-accent">
                {relation.targetText}
              </Link>{' '}
              将进入历史知识（仍可在知识页查看，只是不再作为当前结论）
            </span>
          </p>
          <p className="flex items-start gap-2 text-[13px] leading-relaxed">
            <span className="mt-1.5 h-1.5 w-1.5 shrink-0 rounded-full bg-accent" />
            <span>
              <Link to={`/claims/${relation.sourceClaimId}`} className="hover:text-accent">
                {relation.sourceText}
              </Link>{' '}
              成为当前结论
            </span>
          </p>
        </Card>
      ) : null}
    </div>
  );
}
