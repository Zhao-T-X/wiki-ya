import { useState } from 'react';
import { Link, useNavigate, useParams } from 'react-router-dom';

import { ErrorNotice } from '@/components/ErrorNotice';
import { PageHeader } from '@/components/PageHeader';
import { KnowledgeIcon, PlusIcon, SearchIcon } from '@/components/icons';
import { claimSentence } from '@/components/ClaimCard';
import { Badge, StatusBadge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { EmptyState } from '@/components/ui/EmptyState';
import { Input } from '@/components/ui/Input';
import { Select } from '@/components/ui/Select';
import { Spinner } from '@/components/ui/Spinner';
import { CreateClaimDialog } from '@/features/knowledge/CreateClaimDialog';
import { EntityDetailView } from '@/features/knowledge/EntityDetailView';
import { get_entity, list_claims, list_entities, list_registries, WikiError } from '@/lib/api';
import { cn } from '@/lib/cn';
import { lifecycleLabel } from '@/lib/status';
import { useAsyncData, useDebouncedValue } from '@/lib/hooks';
import type { ClaimCard } from '@/types/ipc';

type Tab = 'knowledge' | 'entity';

const lifecycleTone: Record<string, 'accent' | 'neutral'> = {
  current: 'accent',
  superseded: 'neutral',
  excluded: 'neutral',
};

export function KnowledgePage() {
  const { entityId } = useParams<{ entityId: string }>();
  const navigate = useNavigate();

  const registries = useAsyncData(() => list_registries(), []);
  const [tab, setTab] = useState<Tab>('knowledge');
  const [queryInput, setQueryInput] = useState('');
  const debouncedQuery = useDebouncedValue(queryInput, 250);
  const [entityType, setEntityType] = useState('all');
  const [onlyCurrent, setOnlyCurrent] = useState(true);
  const [dialogOpen, setDialogOpen] = useState(false);

  const entities = useAsyncData(
    () =>
      list_entities({
        query: debouncedQuery.trim() === '' ? undefined : debouncedQuery.trim(),
        entityType: entityType === 'all' ? undefined : entityType,
        limit: 100,
      }),
    [debouncedQuery, entityType],
  );

  // 知识优先：按内容检索 Claim，每条直达富详情页。
  const claims = useAsyncData(
    () =>
      list_claims({
        query: debouncedQuery.trim() === '' ? undefined : debouncedQuery.trim(),
        limit: 200,
      }),
    [debouncedQuery],
  );

  const detail = useAsyncData(
    () => (entityId ? get_entity({ id: entityId, depth: 1 }) : Promise.resolve(null)),
    [entityId],
    Boolean(entityId),
  );

  const entityList = entities.data ?? [];
  const entityTypes = registries.data?.entityTypes ?? [];

  const visibleClaims = (claims.data ?? []).filter((claim) =>
    onlyCurrent ? claim.lifecycle === 'current' : true,
  );

  const refresh = () => {
    detail.reload();
    entities.reload();
    claims.reload();
  };

  return (
    <div>
      <PageHeader
        title="Knowledge"
        subtitle="先找知识（Claim），再看组织它的实体。每条知识都能追到证据、演化与来源。"
        actions={
          <Button
            variant="primary"
            size="sm"
            onClick={() => setDialogOpen(true)}
            title="手动录入一条结构化知识"
          >
            <PlusIcon className="h-3.5 w-3.5" />
            手动新建 Claim
          </Button>
        }
      />

      <div className="mb-4 flex items-center gap-1 rounded-lg border border-line bg-surface p-1 text-xs">
        {([
          ['knowledge', '知识'],
          ['entity', '实体'],
        ] as const).map(([value, label]) => (
          <button
            key={value}
            type="button"
            onClick={() => setTab(value)}
            className={cn(
              'flex-1 rounded-md px-3 py-1.5 font-medium transition-colors',
              tab === value
                ? 'bg-elevated text-ink shadow-sm'
                : 'text-muted hover:text-ink',
            )}
          >
            {label}
          </button>
        ))}
      </div>

      <div className="grid gap-6 lg:grid-cols-[320px_minmax(0,1fr)]">
        <div className="space-y-3">
          <div className="flex gap-2">
            <div className="relative flex-1">
              <SearchIcon className="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted" />
              <Input
                value={queryInput}
                onChange={(event) => setQueryInput(event.target.value)}
                placeholder={tab === 'knowledge' ? '搜索知识…' : '搜索实体…'}
                className="pl-8"
              />
            </div>
            {tab === 'entity' ? (
              <Select
                value={entityType}
                onChange={(event) => setEntityType(event.target.value)}
                className="w-32"
                aria-label="实体类型筛选"
              >
                <option value="all">全部类型</option>
                {entityTypes.map((type) => (
                  <option key={type.value} value={type.value} title={type.description}>
                    {type.value}
                  </option>
                ))}
              </Select>
            ) : (
              <label className="flex items-center gap-1.5 whitespace-nowrap text-meta text-muted">
                <input
                  type="checkbox"
                  checked={onlyCurrent}
                  onChange={(event) => setOnlyCurrent(event.target.checked)}
                  className="accent-accent"
                />
                仅当前
              </label>
            )}
          </div>

          {tab === 'knowledge' ? (
            <KnowledgeList
              claims={visibleClaims}
              loading={claims.loading}
              error={claims.error}
              query={debouncedQuery}
            />
          ) : (
            <EntityList
              entities={entityList}
              loading={entities.loading}
              error={entities.error}
              entityId={entityId}
              onSelect={(id) => navigate(`/knowledge/${id}`)}
            />
          )}
        </div>

        <div className="min-w-0">
          {tab === 'entity' && !entityId ? (
            <EmptyState
              title="选择一个实体"
              description="实体是组织知识的容器：选中后可查看别名、Claim、关系、时间线与邻域图。"
              icon={<KnowledgeIcon className="h-5 w-5" />}
            />
          ) : null}

          {tab === 'knowledge' && !entityId ? (
            <EmptyState
              title="选择一条知识"
              description="在左侧选择一条知识，即可查看它的内容、证据、演化与完整来源。"
              icon={<KnowledgeIcon className="h-5 w-5" />}
            />
          ) : null}

          {detail.loading && !detail.data ? (
            <div className="flex items-center gap-2 px-1 py-6 text-xs text-muted">
              <Spinner className="h-3.5 w-3.5" />
              加载实体详情…
            </div>
          ) : null}

          {detail.error ? <ErrorNotice error={detail.error} /> : null}

          {detail.data ? (
            <EntityDetailView detail={detail.data} onSelectEntity={(id) => navigate(`/knowledge/${id}`)} />
          ) : null}
        </div>
      </div>

      <CreateClaimDialog
        open={dialogOpen}
        onClose={() => setDialogOpen(false)}
        onCreated={refresh}
        registries={registries.data}
        defaultSubject={detail.data?.entity.name ?? ''}
      />
    </div>
  );
}

function KnowledgeList({
  claims,
  loading,
  error,
  query,
}: {
  claims: ClaimCard[];
  loading: boolean;
  error: WikiError | null;
  query: string;
}) {
  if (error) return <ErrorNotice error={error} />;
  if (loading && claims.length === 0) {
    return (
      <div className="flex items-center gap-2 px-1 py-6 text-xs text-muted">
        <Spinner className="h-3.5 w-3.5" />
        加载知识…
      </div>
    );
  }
  if (!loading && claims.length === 0) {
    return (
      <EmptyState
        title={query.trim() ? '没有匹配的知识' : '还没有知识'}
        description={query.trim() ? '换一个关键字，或先抽取 / 录入内容。' : '抽取文档或手动录入一条 Claim。'}
        icon={<KnowledgeIcon className="h-5 w-5" />}
      />
    );
  }

  return (
    <ul className="space-y-1">
      {claims.map((claim) => (
        <li key={claim.id}>
          <Link
            to={`/claims/${claim.id}`}
            className="block rounded-lg border border-transparent px-3 py-2 transition-colors hover:border-line hover:bg-elevated/60"
          >
            <div className="flex items-center justify-between gap-2">
              <span className="truncate text-sm text-ink">{claim.subjectName}</span>
              <Badge tone={lifecycleTone[claim.lifecycle] ?? 'neutral'}>
                {lifecycleLabel(claim.lifecycle)}
              </Badge>
            </div>
            <p className="mt-0.5 text-xs leading-relaxed text-muted">
              {claimSentence(claim)}
            </p>
            <div className="mt-1 flex items-center gap-2 text-[10px] text-muted/70">
              <span>{claim.evidenceCount} 证据</span>
              <span>·</span>
              <StatusBadge status={claim.status} />
            </div>
          </Link>
        </li>
      ))}
    </ul>
  );
}

function EntityList({
  entities,
  loading,
  error,
  entityId,
  onSelect,
}: {
  entities: { id: string; name: string; status: string; primaryType: string; claimCount: number; aliasCount: number }[];
  loading: boolean;
  error: WikiError | null;
  entityId?: string;
  onSelect: (id: string) => void;
}) {
  if (error) return <ErrorNotice error={error} />;
  if (loading && entities.length === 0) {
    return (
      <div className="flex items-center gap-2 px-1 py-6 text-xs text-muted">
        <Spinner className="h-3.5 w-3.5" />
        加载实体…
      </div>
    );
  }
  if (!loading && entities.length === 0) {
    return (
      <EmptyState
        title="没有匹配的实体"
        description="换一个关键字，或先在 Home 捕获内容。"
        icon={<KnowledgeIcon className="h-5 w-5" />}
      />
    );
  }

  return (
    <ul className="space-y-1">
      {entities.map((entity) => {
        const active = entity.id === entityId;
        return (
          <li key={entity.id}>
            <button
              type="button"
              onClick={() => onSelect(entity.id)}
              className={cn(
                'w-full rounded-lg border px-3 py-2 text-left transition-colors',
                active
                  ? 'border-accent/40 bg-elevated'
                  : 'border-transparent hover:border-line hover:bg-elevated/60',
              )}
            >
              <div className="flex items-center justify-between gap-2">
                <span className="truncate text-sm text-ink">{entity.name}</span>
                <StatusBadge status={entity.status} />
              </div>
              <div className="mt-1 flex items-center gap-2 text-[10px] text-muted">
                <Badge tone="accent">{entity.primaryType}</Badge>
                <span>{entity.claimCount} claims</span>
                <span>·</span>
                <span>{entity.aliasCount} aliases</span>
              </div>
            </button>
          </li>
        );
      })}
    </ul>
  );
}
