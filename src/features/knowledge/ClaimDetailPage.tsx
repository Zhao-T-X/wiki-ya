import { Link, useParams } from 'react-router-dom';

import { EvidenceList } from '@/components/EvidenceList';
import { ErrorNotice } from '@/components/ErrorNotice';
import { PageHeader } from '@/components/PageHeader';
import { claimSentence } from '@/components/ClaimCard';
import { ArrowRightIcon } from '@/components/icons';
import { Badge, StatusBadge } from '@/components/ui/Badge';
import { Card } from '@/components/ui/Card';
import { EmptyState } from '@/components/ui/EmptyState';
import { Spinner } from '@/components/ui/Spinner';
import { get_claim, get_claim_history } from '@/lib/api';
import { formatConfidence, formatDateTime, humanizePredicate } from '@/lib/format';
import { useAsyncData } from '@/lib/hooks';
import { claimTypeLabel, lifecycleLabel, modalityLabel, polarityLabel, relationshipLabel, relationshipTone } from '@/lib/status';

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

  return (
    <div>
      <PageHeader
        title="Claim 详情"
        subtitle={
          <Link to="/knowledge" className="inline-flex items-center gap-1 text-accent hover:underline">
            返回 Knowledge <ArrowRightIcon className="h-3 w-3" />
          </Link>
        }
      />

      {detail.loading && !detail.data ? (
        <div className="flex items-center gap-2 py-6 text-xs text-muted">
          <Spinner className="h-3.5 w-3.5" />
          加载 Claim…
        </div>
      ) : null}

      {detail.error ? <ErrorNotice error={detail.error} /> : null}

      {!detail.loading && !detail.error && !detail.data ? (
        <EmptyState title="未找到该 Claim" description="它可能已被删除，或链接已失效。" />
      ) : null}

      {claim && detail.data ? (
        <div className="space-y-6">
          <Card className="p-4">
            <div className="flex items-start justify-between gap-3">
              <p className="text-base leading-snug text-ink">{claimSentence(claim)}</p>
              <StatusBadge status={claim.status} />
            </div>

            <dl className="mt-3 grid grid-cols-2 gap-x-6 gap-y-2 text-[11px] text-muted sm:grid-cols-3">
              <div>
                <dt className="text-muted/70">Subject</dt>
                <dd className="text-ink/90">{claim.subjectName}</dd>
              </div>
              <div>
                <dt className="text-muted/70">Predicate</dt>
                <dd className="font-mono text-ink/90">{humanizePredicate(claim.predicate)}</dd>
              </div>
              <div>
                <dt className="text-muted/70">Object</dt>
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
              <div>
                <dt className="text-muted/70">Lifecycle</dt>
                <dd className="text-ink/90">{lifecycleLabel(claim.lifecycle)}</dd>
              </div>
            </dl>

            {claim.sourceQuote ? (
              <blockquote className="mt-3 border-l-2 border-line pl-3 text-xs italic leading-relaxed text-muted">
                {claim.sourceQuote}
              </blockquote>
            ) : null}
          </Card>

          <section>
            <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-muted">
              Evidence（{detail.data.evidence.length}）
            </h3>
            <EvidenceList evidence={detail.data.evidence} />
          </section>

          <section>
            <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-muted">
              演化关系（accepted 优先）
            </h3>
            {detail.data.relations.length === 0 ? (
              <p className="text-xs text-muted">暂无已确认的演化关系。</p>
            ) : (
              <ul className="space-y-2">
                {detail.data.relations.map((relation) => (
                  <li key={relation.id} className="rounded-lg border border-line bg-surface p-3 text-xs">
                    <Badge tone={relationshipTone(relation.relationship)}>
                      {relationshipLabel(relation.relationship)}
                    </Badge>
                    <p className="mt-2 leading-relaxed text-muted">
                      <span className="text-ink/90">{relation.sourceText}</span>
                      <span className="mx-2 text-muted">→</span>
                      <span className="text-ink/90">{relation.targetText}</span>
                    </p>
                    {relation.reason ? <p className="mt-1 text-[11px] text-muted/80">{relation.reason}</p> : null}
                  </li>
                ))}
              </ul>
            )}
          </section>

          <section>
            <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-muted">
              History（时间倒序）
            </h3>
            {history.error ? <ErrorNotice error={history.error} /> : null}
            {history.loading && !history.data ? (
              <div className="flex items-center gap-2 text-xs text-muted">
                <Spinner className="h-3.5 w-3.5" />
                加载历史…
              </div>
            ) : null}
            {history.data && history.data.length === 0 ? (
              <p className="text-xs text-muted">该 Claim 尚无演化历史。</p>
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
        </div>
      ) : null}
    </div>
  );
}
