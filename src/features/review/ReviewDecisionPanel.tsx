import { Link } from 'react-router-dom';

import { ReviewEvidence } from '@/features/review/ReviewEvidence';
import { ReviewImpact } from '@/features/review/ReviewImpact';
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from '@/components/ui/Accordion';
import { Badge, StatusBadge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { formatConfidence } from '@/lib/format';
import { relationshipLabel, relationshipTone } from '@/lib/status';
import type { ClaimRelationDecision, ReviewItem } from '@/types/ipc';

/** 契约映射（任务书 §22）：界面用中文，IPC 语义仍是 accept / reset / reject。 */
const DECISIONS: {
  label: string;
  decision: ClaimRelationDecision;
  variant: 'primary' | 'secondary' | 'danger';
  hint: string;
}[] = [
  { label: '接受建议', decision: 'accept', variant: 'primary', hint: '接受这条关系（取代会让旧知识进入历史）' },
  { label: '保留两者', decision: 'reset', variant: 'secondary', hint: '回退该关系，两者都作为当前知识保留' },
  { label: '拒绝', decision: 'reject', variant: 'danger', hint: '拒绝该关系，不改动任何知识' },
];

function Section({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section className="space-y-1.5">
      <h3 className="text-[11px] font-semibold uppercase tracking-wider text-muted">{title}</h3>
      {children}
    </section>
  );
}

/**
 * 决策面板（任务书 §21）。
 *
 * 固定四段：**发生了什么 / 为什么 / 证据 / 影响**。用户从上往下读一遍就能
 * 做完决定，不需要在多个折叠区之间来回跳。
 *
 * 技术指标（优先级 / 置信度 / 建议动作 / 规则说明）收进底部的「为什么这样判断 ▾」
 * ——此前它们摊在卡片顶部，与「我正在决定什么」抢注意力（任务书 §21）。
 */
export function ReviewDecisionPanel({
  item,
  busy,
  onDecide,
}: {
  item: ReviewItem;
  busy: boolean;
  onDecide: (decision: ClaimRelationDecision) => void;
}) {
  const { relation } = item;

  return (
    <div className="flex h-full flex-col">
      <div className="min-h-0 flex-1 space-y-5 overflow-y-auto pr-1">
        <div className="flex flex-wrap items-center gap-2">
          <Badge tone={relationshipTone(relation.relationship)}>
            {relationshipLabel(relation.relationship)}
          </Badge>
          <StatusBadge status={relation.status} />
        </div>

        {/* ① 发生了什么：像知识 diff，而不是两张并列的卡片 */}
        <Section title="发生了什么">
          <div className="space-y-2">
            <Card tone="quiet" className="border-l-2 border-l-accent p-3">
              <p className="text-[11px] uppercase tracking-wider text-accent">新知识</p>
              <p className="mt-1 text-body leading-relaxed text-ink">{relation.sourceText}</p>
              <Link
                to={`/claims/${relation.sourceClaimId}`}
                className="mt-1.5 inline-block text-[11px] text-accent hover:underline"
              >
                查看详情
              </Link>
            </Card>

            <div className="flex justify-center text-[11px] text-muted">vs</div>

            <Card tone="quiet" className="border-l-2 border-l-muted/50 p-3">
              <p className="text-[11px] uppercase tracking-wider text-muted">已有知识</p>
              <p className="mt-1 text-body leading-relaxed text-ink/90">{relation.targetText}</p>
              <Link
                to={`/claims/${relation.targetClaimId}`}
                className="mt-1.5 inline-block text-[11px] text-accent hover:underline"
              >
                查看详情
              </Link>
            </Card>
          </div>
        </Section>

        {/* ② 为什么：item.why 与 relation.reason 合并展示，避免重复 */}
        <Section title="为什么需要你决定">
          <Card tone="quiet" className="p-3">
            <p className="text-[13px] leading-relaxed text-ink/90">{item.whatChanged}</p>
            <p className="mt-2 text-[13px] leading-relaxed text-muted">{item.why}</p>
          </Card>
        </Section>

        {/* ③ 证据 */}
        <Section title="证据">
          <ReviewEvidence quote={item.evidenceQuote} />
        </Section>

        {/* ④ 影响 */}
        <Section title="影响">
          <ReviewImpact item={item} />
        </Section>

        <Accordion type="single" collapsible>
          <AccordionItem value="why-judged">
            <AccordionTrigger>为什么这样判断</AccordionTrigger>
            <AccordionContent>
              <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
                <dt className="text-muted">优先级</dt>
                <dd className="font-mono text-ink/80">{item.priority}</dd>

                <dt className="text-muted">置信度</dt>
                <dd className="font-mono text-ink/80">{formatConfidence(relation.confidence)}</dd>

                {relation.suggestedAction ? (
                  <>
                    <dt className="text-muted">建议动作</dt>
                    <dd className="text-ink/80">{relation.suggestedAction}</dd>
                  </>
                ) : null}

                {relation.reason ? (
                  <>
                    <dt className="text-muted">规则说明</dt>
                    <dd className="leading-relaxed text-ink/80">{relation.reason}</dd>
                  </>
                ) : null}
              </dl>
            </AccordionContent>
          </AccordionItem>
        </Accordion>
      </div>

      {/* ③ 操作区 sticky：看完长证据后不必滚回顶部找按钮（任务书 §13）。 */}
      <div className="sticky bottom-0 mt-4 border-t border-line bg-surface/95 pt-3 backdrop-blur">
        <div className="flex flex-wrap gap-2">
          {DECISIONS.map((option) => (
            <Button
              key={option.decision}
              variant={option.variant}
              title={option.hint}
              loading={busy}
              disabled={busy}
              onClick={() => onDecide(option.decision)}
            >
              {option.label}
            </Button>
          ))}
        </div>
      </div>
    </div>
  );
}
