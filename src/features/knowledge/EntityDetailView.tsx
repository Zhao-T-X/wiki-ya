import { useMemo } from 'react';

import { GraphCanvas } from '@/components/GraphCanvas';
import { Badge, StatusBadge } from '@/components/ui/Badge';
import { claimSentence } from '@/components/ClaimCard';
import { ClockIcon } from '@/components/icons';
import { ClaimList } from '@/features/knowledge/ClaimList';
import { formatConfidence, formatRelativeTime, humanizePredicate, yearOf } from '@/lib/format';
import type { ClaimCard, EntityDetail } from '@/types/ipc';

export interface EntityDetailViewProps {
  detail: EntityDetail;
  onSelectEntity: (id: string) => void;
}

function SectionTitle({ children, hint }: { children: string; hint?: string }) {
  return (
    <div className="mb-3 flex items-baseline justify-between gap-2">
      <h3 className="text-[11px] font-semibold uppercase tracking-wider text-muted">{children}</h3>
      {hint ? <span className="text-[11px] text-muted/70">{hint}</span> : null}
    </div>
  );
}

export function EntityDetailView({ detail, onSelectEntity }: EntityDetailViewProps) {
  const { entity } = detail;

  const timeline = useMemo(() => {
    const groups = new Map<number, ClaimCard[]>();
    for (const claim of detail.claims) {
      const year = yearOf(claim.createdAt);
      const key = year ?? 0;
      const list = groups.get(key);
      if (list) list.push(claim);
      else groups.set(key, [claim]);
    }
    return [...groups.entries()]
      .sort((a, b) => b[0] - a[0])
      .map(([year, claims]) => ({
        year,
        claims: [...claims].sort((a, b) => b.createdAt.localeCompare(a.createdAt)),
      }));
  }, [detail.claims]);

  return (
    <div className="space-y-7">
      <header>
        <div className="flex flex-wrap items-center gap-2">
          <h2 className="text-lg font-semibold tracking-tight text-ink">{entity.name}</h2>
          <StatusBadge status={entity.status} />
          {entity.types.map((type) => (
            <Badge key={type} tone={type === entity.primaryType ? 'accent' : 'neutral'}>
              {type}
            </Badge>
          ))}
        </div>
        {entity.description ? (
          <p className="mt-2 max-w-2xl text-sm leading-relaxed text-muted">{entity.description}</p>
        ) : null}
        <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-muted">
          <span>{entity.aliasCount} 个别名</span>
          <span className="text-line">·</span>
          <span>{entity.claimCount} 条 Claim</span>
          <span className="text-line">·</span>
          <span>{detail.relations.length} 条关系</span>
        </div>
      </header>

      {detail.aliases.length > 0 ? (
        <section>
          <SectionTitle>别名</SectionTitle>
          <div className="flex flex-wrap gap-1.5">
            {detail.aliases.map((alias) => (
              <Badge key={alias}>{alias}</Badge>
            ))}
          </div>
        </section>
      ) : null}

      <section>
        <SectionTitle hint="点击展开证据">Claims</SectionTitle>
        <ClaimList claims={detail.claims} emptyText="该实体暂无 Claim。可在右上角手动新建。" />
      </section>

      <section>
        <SectionTitle>Relations</SectionTitle>
        {detail.relations.length === 0 ? (
          <p className="text-xs text-muted">该实体暂无实体间关系。</p>
        ) : (
          <ul className="space-y-1.5">
            {detail.relations.map((relation) => (
              <li
                key={relation.id}
                className="flex flex-wrap items-center gap-2 rounded-lg border border-line bg-surface px-3 py-2 text-xs"
              >
                <button
                  type="button"
                  onClick={() => onSelectEntity(relation.sourceId)}
                  className="text-ink transition-colors hover:text-accent"
                >
                  {relation.sourceName}
                </button>
                <span className="font-mono text-[11px] text-accent">{relation.predicate}</span>
                <span className="text-muted">→</span>
                <button
                  type="button"
                  onClick={() => onSelectEntity(relation.targetId)}
                  className="text-ink transition-colors hover:text-accent"
                >
                  {relation.targetName}
                </button>
                {relation.inverseLabel ? (
                  <span className="text-[10px] text-muted/70">（反向：{relation.inverseLabel}）</span>
                ) : null}
                <span className="ml-auto flex items-center gap-2">
                  <span className="text-[10px] text-muted">置信度 {formatConfidence(relation.confidence)}</span>
                  <StatusBadge status={relation.status} />
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section>
        <SectionTitle hint="按 createdAt 分组">Timeline</SectionTitle>
        {timeline.length === 0 ? (
          <p className="text-xs text-muted">暂无时间线数据。</p>
        ) : (
          <ol className="space-y-4 border-l border-line pl-4">
            {timeline.map((group) => (
              <li key={group.year} className="relative">
                <span className="absolute -left-[21px] top-1.5 h-2 w-2 rounded-full bg-accent/70" />
                <p className="flex items-center gap-1.5 text-xs font-semibold text-ink">
                  <ClockIcon className="h-3.5 w-3.5 text-muted" />
                  {group.year === 0 ? '时间未知' : group.year}
                </p>
                <ul className="mt-1.5 space-y-1">
                  {group.claims.map((claim) => (
                    <li key={claim.id} className="text-xs leading-relaxed text-muted">
                      {claimSentence(claim)}
                      <span className="ml-2 font-mono text-[10px] text-muted/60">
                        {humanizePredicate(claim.predicate)}
                      </span>
                      <span className="ml-2 text-[10px] text-muted/60">
                        {formatRelativeTime(claim.createdAt)}
                      </span>
                    </li>
                  ))}
                </ul>
              </li>
            ))}
          </ol>
        )}
      </section>

      <section>
        <SectionTitle hint={`${detail.graph.nodes.length} 节点 / ${detail.graph.edges.length} 边`}>
          邻域图
        </SectionTitle>
        <GraphCanvas
          nodes={detail.graph.nodes}
          edges={detail.graph.edges}
          activeId={entity.id}
          onSelect={onSelectEntity}
          height={380}
        />
      </section>
    </div>
  );
}
