import { useMemo, useState } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';

import { ErrorNotice } from '@/components/ErrorNotice';
import { GraphCanvas } from '@/components/GraphCanvas';
import { PageHeader } from '@/components/PageHeader';
import { GraphIcon, SearchIcon } from '@/components/icons';
import { Badge, StatusBadge } from '@/components/ui/Badge';
import { EmptyState } from '@/components/ui/EmptyState';
import { Input } from '@/components/ui/Input';
import { Select } from '@/components/ui/Select';
import { Spinner } from '@/components/ui/Spinner';
import { get_entity, list_entities, list_registries } from '@/lib/api';
import { cn } from '@/lib/cn';
import { useAsyncData, useDebouncedValue } from '@/lib/hooks';

const DEPTH_OPTIONS = [1, 2, 3];

export function GraphPage() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();

  const registries = useAsyncData(() => list_registries(), []);

  const [queryInput, setQueryInput] = useState('');
  const debouncedQuery = useDebouncedValue(queryInput, 250);
  const [rootId, setRootId] = useState<string | null>(searchParams.get('entity'));
  const [depth, setDepth] = useState(1);
  const [predicate, setPredicate] = useState('all');

  const entities = useAsyncData(
    () => list_entities({ query: debouncedQuery.trim(), limit: 20 }),
    [debouncedQuery],
    debouncedQuery.trim().length > 0,
  );

  const graph = useAsyncData(
    () => (rootId ? get_entity({ id: rootId, depth }) : Promise.resolve(null)),
    [rootId, depth],
    Boolean(rootId),
  );

  const rawGraph = graph.data?.graph ?? null;

  const { nodes, edges } = useMemo(() => {
    if (!rawGraph) return { nodes: [], edges: [] };
    if (predicate === 'all') return { nodes: rawGraph.nodes, edges: rawGraph.edges };

    const filteredEdges = rawGraph.edges.filter((edge) => edge.predicate === predicate);
    const keep = new Set<string>();
    if (rootId) keep.add(rootId);
    for (const edge of filteredEdges) {
      keep.add(edge.source);
      keep.add(edge.target);
    }
    return { nodes: rawGraph.nodes.filter((node) => keep.has(node.id)), edges: filteredEdges };
  }, [rawGraph, predicate, rootId]);

  const relationPredicates = registries.data?.relationPredicates ?? [];
  const entityMatches = entities.data ?? [];

  return (
    <div>
      <PageHeader
        title="Graph"
        subtitle="默认 Entity Graph，只渲染所选实体的邻域（确定性径向布局），不做全图加载。"
      />

      <div className="grid gap-6 lg:grid-cols-[300px_minmax(0,1fr)]">
        <div className="space-y-4">
          <div>
            <label className="mb-1.5 block text-[11px] font-medium text-muted">根实体</label>
            <div className="relative">
              <SearchIcon className="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted" />
              <Input
                value={queryInput}
                onChange={(event) => setQueryInput(event.target.value)}
                placeholder="搜索实体作为中心…"
                className="pl-8"
              />
            </div>
          </div>

          {entities.error ? <ErrorNotice error={entities.error} /> : null}

          {queryInput.trim() !== '' ? (
            <div className="space-y-1">
              {entities.loading ? (
                <div className="flex items-center gap-2 px-1 py-2 text-xs text-muted">
                  <Spinner className="h-3.5 w-3.5" />
                  搜索中…
                </div>
              ) : null}
              {entityMatches.map((entity) => (
                <button
                  key={entity.id}
                  type="button"
                  onClick={() => setRootId(entity.id)}
                  className={cn(
                    'w-full rounded-lg border px-3 py-2 text-left text-xs transition-colors',
                    entity.id === rootId
                      ? 'border-accent/40 bg-elevated text-ink'
                      : 'border-line bg-surface text-muted hover:text-ink',
                  )}
                >
                  <span className="block truncate">{entity.name}</span>
                  <span className="mt-0.5 block text-[10px] text-muted/70">
                    {entity.primaryType} · {entity.claimCount} claims
                  </span>
                </button>
              ))}
              {!entities.loading && entityMatches.length === 0 ? (
                <p className="px-1 py-2 text-xs text-muted">没有匹配的实体。</p>
              ) : null}
            </div>
          ) : null}

          <div className="grid grid-cols-2 gap-2">
            <div>
              <label className="mb-1.5 block text-[11px] font-medium text-muted" htmlFor="graph-depth">
                Depth
              </label>
              <Select
                id="graph-depth"
                value={String(depth)}
                onChange={(event) => setDepth(Number(event.target.value))}
              >
                {DEPTH_OPTIONS.map((option) => (
                  <option key={option} value={option}>
                    {option} 层
                  </option>
                ))}
              </Select>
            </div>
            <div>
              <label className="mb-1.5 block text-[11px] font-medium text-muted" htmlFor="graph-predicate">
                Predicate
              </label>
              <Select
                id="graph-predicate"
                value={predicate}
                onChange={(event) => setPredicate(event.target.value)}
              >
                <option value="all">全部</option>
                {relationPredicates.map((item) => (
                  <option key={item.predicate} value={item.predicate}>
                    {item.predicate}
                  </option>
                ))}
              </Select>
            </div>
          </div>

          {graph.data ? (
            <div className="rounded-lg border border-line bg-surface p-3">
              <div className="flex items-center gap-2">
                <StatusBadge status={graph.data.entity.status} />
                <span className="truncate text-sm text-ink">{graph.data.entity.name}</span>
              </div>
              <div className="mt-1.5 flex flex-wrap gap-1.5">
                {graph.data.entity.types.map((type) => (
                  <Badge key={type} tone={type === graph.data?.entity.primaryType ? 'accent' : 'neutral'}>
                    {type}
                  </Badge>
                ))}
              </div>
              <p className="mt-2 text-[10px] text-muted">
                {nodes.length} 节点 / {edges.length} 边
              </p>
            </div>
          ) : null}
        </div>

        <div className="min-w-0">
          {!rootId ? (
            <EmptyState
              title="选择一个根实体"
              description="Graph 通过 get_entity 读取邻域。先搜索并选中一个实体，再调整 depth 与 predicate。"
              icon={<GraphIcon className="h-5 w-5" />}
            />
          ) : null}

          {graph.error ? <ErrorNotice error={graph.error} /> : null}

          {graph.loading && !graph.data ? (
            <div className="flex items-center gap-2 py-6 text-xs text-muted">
              <Spinner className="h-3.5 w-3.5" />
              加载邻域图…
            </div>
          ) : null}

          {rootId && graph.data ? (
            <GraphCanvas
              nodes={nodes}
              edges={edges}
              activeId={rootId}
              onSelect={(id) => navigate(`/knowledge/${id}`)}
              height={520}
            />
          ) : null}

          {rootId && graph.data ? (
            <p className="mt-3 text-[11px] text-muted">
              提示：点击任意节点可跳转到该实体的 Knowledge 详情。
            </p>
          ) : null}
        </div>
      </div>
    </div>
  );
}
