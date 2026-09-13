import { useMemo } from 'react';
import { Link, useParams } from 'react-router-dom';

import { claimSentence } from '@/components/ClaimCard';
import { EvidenceList } from '@/components/EvidenceList';
import { ErrorNotice } from '@/components/ErrorNotice';
import { PageHeader } from '@/components/PageHeader';
import { ArrowRightIcon } from '@/components/icons';
import { Badge, StatusBadge } from '@/components/ui/Badge';
import { Card } from '@/components/ui/Card';
import { EmptyState } from '@/components/ui/EmptyState';
import { Spinner } from '@/components/ui/Spinner';
import { get_claim, get_claim_history } from '@/lib/api';
import { formatConfidence, formatDateTime, humanizePredicate } from '@/lib/format';
import { useAsyncData } from '@/lib/hooks';
import {
  claimTypeLabel,
  lifecycleLabel,
  modalityLabel,
  polarityLabel,
  relationshipLabel,
  relationshipTone,
} from '@/lib/status';

/**
 * Knowledge Detail（UX 重构）。
 *
 * 页面按用户真正会问的问题组织，而不是按数据表组织：
 *   What        —— 这条知识是什么
 *   Why current —— 为什么它现在是"当前知识"（全系统统一的「为什么」交互）
 *   Evidence    —— 我凭什么知道
 *   History     —— 以前是什么
 *   Related     —— 还和什么有关
 * 内部字段（Predicate / 置信度 / 模态…）收进「技术细节」，默认不打扰用户。
 */
export function ClaimDetailPage() {
  const { claimId } = useParams<{ claimId: string }>();

  const detail = useAsyncData(
    () => (claimId ? get_claim({ id: claimId }) : Promise.resolve(null)),
    [claimId],
    Boolean(claimId),
  );

  const history = useAsyncData(
    () => (claimId ? get_claim_history({ id: claimId }) : Promise.resolve([])),
    [claimId],
    Boolean(claimId),
  );

  const claim = detail.data?.claim ?? null;

  /** 其他相关知识：从已确认的演化关系里取"对面的那条"。 */
  const related = useMemo(() => {
    if (!claim || !detail.data) return [];
    const map = new Map<string, { id: string; text: string; relationship: string; status: string }>();
    for (const relation of detail.data.relations) {
      const other =
        relation.sourceClaimId === claim.id
          ? { id: relation.targetClaimId, text: relation.targetText }
          : relation.targetClaimId === claim.id
            ? { id: relation.sourceClaimId, text: relation.sourceText }
            : null;
      if (other && other.id !== claim.id && !map.has(other.id)) {
        map.set(other.id, {
          ...other,
          relationship: relation.relationship,
          status: relation.status,
        });
      }
    }
    return [...map.values()];
  }, [claim, detail.data]);

  /** 取代了哪些旧知识 / 被谁取代。 */
  const { replacedOld, replacedBy } = useMemo(() => {
    const out = { replacedOld: [] as string[], replacedBy: [] as string[] };
    if (!claim || !detail.data) return out;
    for (const relation of detail.data.relations) {
      if (relation.relationship !== 'supersedes' || relation.status !== 'accepted') continue;
      if (relation.sourceClaimId === claim.id) out.replacedOld.push(relation.targetText);
      if (relation.targetClaimId === claim.id) out.replacedBy.push(relation.sourceText);
    }
    return out;
  }, [claim, detail.data]);

  return (
    <div>
      <PageHeader
        title="知识详情"
        subtitle={
          <Link to="/knowledge" className="inline-flex items-center gap-1 text-accent hover:underline">
            返回 Knowledge <ArrowRightIcon className="h-3 w-3" />
          </Link>
        }
      />

      {detail.loading && !detail.data ? (
        <div className="flex items-center gap-2 py-6 text-xs text-muted">
          <Spinner className="h-3.5 w-3.5" />
          加载中…
        </div>
      ) : null}

      {detail.error ? <ErrorNotice error={detail.error} /> : null}

      {!detail.loading && !detail.error && !detail.data ? (
        <EmptyState title="未找到该知识" description="它可能已被删除，或链接已失效。" />
      ) : null}

      {claim && detail.data ? (
        <div className="space-y-6">
          {/* What */}
          <Card className="p-5">
            <div className="flex items-start justify-between gap-3">
              <p className="text-lg leading-snug text-ink">{claimSentence(claim)}</p>
              <StatusBadge status={claim.status} />
            </div>
            <p className="mt-2 text-xs text-muted">{lifecycleLabel(claim.lifecycle)}</p>

            {claim.sourceQuote ? (
              <blockquote className="mt-3 border-l-2 border-line pl-3 text-xs italic leading-relaxed text-muted">
                {claim.sourceQuote}
              </blockquote>
            ) : null}
          </Card>

          {/* Why current —— 全系统统一的「为什么」 */}
          <section>
            <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-muted">
              为什么是当前知识
            </h3>
            <Card className="p-4">
              <ul className="space-y-2 text-xs leading-relaxed text-muted">
                <li className="flex gap-2">
                  <span className="text-accent">•</span>
                  <span>
                    {claim.lifecycle === 'current'
                      ? '它没有被更新、更权威的知识取代，因此是当前生效的知识。'
                      : claim.lifecycle === 'superseded'
                        ? '它已被更新的知识取代，仅作为历史保留。'
                        : '它不参与当前知识（已拒绝 / 已归档 / 草稿）。'}
                  </span>
                </li>

                {replacedOld.length > 0 ? (
                  <li className="flex gap-2">
                    <span className="text-accent">•</span>
                    <span>
                      它取代了 {replacedOld.length} 条旧知识：
                      <span className="text-ink/90"> {replacedOld.join('；')}</span>
                    </span>
                  </li>
                ) : null}

                {replacedBy.length > 0 ? (
                  <li className="flex gap-2">
                    <span className="text-accent">•</span>
                    <span>
                      它被更新的知识取代：
                      <span className="text-ink/90"> {replacedBy.join('；')}</span>
                    </span>
                  </li>
                ) : null}

                <li className="flex gap-2">
                  <span className="text-accent">•</span>
                  <span>
                    {detail.data.evidence.length > 0
                      ? `有 ${detail.data.evidence.length} 条来源证据支持它。`
                      : '目前没有来源证据支持它，可信度较低。'}
                  </span>
                </li>

                {claim.status === 'verified' ? (
                  <li className="flex gap-2">
                    <span className="text-accent">•</span>
                    <span>它已被人工确认。</span>
                  </li>
                ) : null}
              </ul>
            </Card>
          </section>

          {/* Evidence */}
          <section>
            <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-muted">
              来源证据（{detail.data.evidence.length}）
            </h3>
            <EvidenceList evidence={detail.data.evidence} />
          </section>

          {/* History */}
          <section>
            <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-muted">历史</h3>
            {history.error ? <ErrorNotice error={history.error} /> : null}
            {history.loading && !history.data ? (
              <div className="flex items-center gap-2 text-xs text-muted">
                <Spinner className="h-3.5 w-3.5" />
                加载历史…
              </div>
            ) : null}
            {history.data && history.data.length === 0 ? (
              <p className="text-xs text-muted">这条知识还没有变化记录。</p>
            ) : null}
            {history.data && history.data.length > 0 ? (
              <ul className="space-y-2">
                {history.data.map((relation) => (
                  <li
                    key={relation.id}
                    className="flex flex-wrap items-center gap-2 rounded-lg border border-line bg-surface px-3 py-2 text-[11px] text-muted"
                  >
                    <Badge tone={relationshipTone(relation.relationship)}>
                      {relationshipLabel(relation.relationship)}
                    </Badge>
                    <StatusBadge status={relation.status} />
                    <span className="ml-auto">{formatDateTime(relation.createdAt)}</span>
                  </li>
                ))}
              </ul>
            ) : null}
          </section>

          {/* Related */}
          <section>
            <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-muted">相关</h3>
            <div className="flex flex-wrap gap-2">
              <Link
                to={`/knowledge/${claim.subjectId}`}
                className="rounded-md border border-line bg-elevated px-2.5 py-1 text-[11px] text-ink/90 transition-colors hover:border-accent/40 hover:text-accent"
              >
                {claim.subjectName}
              </Link>
              {claim.objectId && claim.objectName ? (
                <Link
                  to={`/knowledge/${claim.objectId}`}
                  className="rounded-md border border-line bg-elevated px-2.5 py-1 text-[11px] text-ink/90 transition-colors hover:border-accent/40 hover:text-accent"
                >
                  {claim.objectName}
                </Link>
              ) : null}
            </div>

            {related.length > 0 ? (
              <ul className="mt-3 space-y-2">
                {related.map((item) => (
                  <li key={item.id}>
                    <Link
                      to={`/claims/${item.id}`}
                      className="flex items-start gap-2 rounded-lg border border-line bg-surface p-3 text-xs transition-colors hover:border-accent/40"
                    >
                      <Badge tone={relationshipTone(item.relationship)}>
                        {relationshipLabel(item.relationship)}
                      </Badge>
                      <span className="min-w-0 flex-1 text-ink/90">{item.text}</span>
                    </Link>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="mt-2 text-xs text-muted">暂无关联知识。</p>
            )}
          </section>

          {/* 技术细节：默认收起，不打扰日常使用 */}
          <details className="rounded-xl border border-line bg-surface px-4 py-3">
            <summary className="cursor-pointer text-[11px] font-semibold uppercase tracking-wider text-muted">
              技术细节
            </summary>
            <dl className="mt-3 grid grid-cols-2 gap-x-6 gap-y-2 text-[11px] text-muted sm:grid-cols-3">
              <div>
                <dt className="text-muted/70">主语</dt>
                <dd className="text-ink/90">{claim.subjectName}</dd>
              </div>
              <div>
                <dt className="text-muted/70">谓语</dt>
                <dd className="font-mono text-ink/90">{humanizePredicate(claim.predicate)}</dd>
              </div>
              <div>
                <dt className="text-muted/70">宾语</dt>
                <dd className="text-ink/90">{claim.objectName ?? claim.objectText ?? '—'}</dd>
              </div>
              <div>
                <dt className="text-muted/70">类型</dt>
                <dd className="text-ink/90">{claimTypeLabel(claim.claimType)}</dd>
              </div>
              <div>
                <dt className="text-muted/70">极性 / 模态</dt>
                <dd className="text-ink/90">
                  {polarityLabel(claim.polarity)} / {modalityLabel(claim.modality)}
                </dd>
              </div>
              <div>
                <dt className="text-muted/70">置信度</dt>
                <dd className="text-ink/90">{formatConfidence(claim.confidence)}</dd>
              </div>
              <div>
                <dt className="text-muted/70">来源文档</dt>
                <dd className="text-ink/90">
                  {claim.sourceDocumentId && claim.sourceDocumentTitle ? (
                    <Link to={`/documents/${claim.sourceDocumentId}`} className="text-accent hover:underline">
                      {claim.sourceDocumentTitle}
                    </Link>
                  ) : (
                    <span className="text-muted">未关联文档</span>
                  )}
                </dd>
              </div>
              <div>
                <dt className="text-muted/70">创建时间</dt>
                <dd className="text-ink/90">{formatDateTime(claim.createdAt)}</dd>
              </div>
              {claim.observedAt ? (
                <div>
                  <dt className="text-muted/70">观察时间</dt>
                  <dd className="text-ink/90">{formatDateTime(claim.observedAt)}</dd>
                </div>
              ) : null}
              <div>
                <dt className="text-muted/70">ID</dt>
                <dd className="font-mono text-ink/90">{claim.id}</dd>
              </div>
            </dl>
          </details>
        </div>
      ) : null}
    </div>
  );
}
