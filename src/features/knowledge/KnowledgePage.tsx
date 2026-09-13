import { useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';

import { ErrorNotice } from '@/components/ErrorNotice';
import { PageHeader } from '@/components/PageHeader';
import { KnowledgeIcon, PlusIcon, SearchIcon } from '@/components/icons';
import { Badge, StatusBadge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { EmptyState } from '@/components/ui/EmptyState';
import { Input } from '@/components/ui/Input';
import { Select } from '@/components/ui/Select';
import { Spinner } from '@/components/ui/Spinner';
import { CreateClaimDialog } from '@/features/knowledge/CreateClaimDialog';
import { EntityDetailView } from '@/features/knowledge/EntityDetailView';
import { get_entity, list_entities, list_registries } from '@/lib/api';
import { cn } from '@/lib/cn';
import { useAsyncData, useDebouncedValue } from '@/lib/hooks';

export function KnowledgePage() {
  const { entityId } = useParams<{ entityId: string }>();
  const navigate = useNavigate();

  const registries = useAsyncData(() => list_registries(), []);
  const [queryInput, setQueryInput] = useState('');
  const debouncedQuery = useDebouncedValue(queryInput, 250);
  const [entityType, setEntityType] = useState('all');
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

  const detail = useAsyncData(
    () => (entityId ? get_entity({ id: entityId, depth: 1 }) : Promise.resolve(null)),
    [entityId],
    Boolean(entityId),
  );

  const entityList = entities.data ?? [];
  const entityTypes = registries.data?.entityTypes ?? [];

  const refresh = () => {
    detail.reload();
    entities.reload();
  };

  return (
    <div>
      <PageHeader
        title="Knowledge"
        subtitle="实体、Claim 与实体间关系。AI 抽取未启用时，可在此手动录入结构化知识。"
        actions={
          <Button
            variant="primary"
            size="sm"
            disabled={!entityId}
            title={entityId ? undefined : '请先在左侧选择一个实体'}
            onClick={() => setDialogOpen(true)}
          >
            <PlusIcon className="h-3.5 w-3.5" />
            手动新建 Claim
          </Button>
        }
      />

      <div className="grid gap-6 lg:grid-cols-[320px_minmax(0,1fr)]">
        <div className="space-y-3">
          <div className="flex gap-2">
            <div className="relative flex-1">
              <SearchIcon className="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted" />
              <Input
                value={queryInput}
                onChange={(event) => setQueryInput(event.target.value)}
                placeholder="搜索实体…"
                className="pl-8"
              />
            </div>
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
          </div>

          {entities.error ? <ErrorNotice error={entities.error} /> : null}

          {entities.loading && !entities.data ? (
            <div className="flex items-center gap-2 px-1 py-6 text-xs text-muted">
              <Spinner className="h-3.5 w-3.5" />
              加载实体…
            </div>
          ) : null}

          {!entities.loading && entityList.length === 0 ? (
            <EmptyState
              title="没有匹配的实体"
              description="换一个关键字，或先在 Home 捕获内容。"
              icon={<KnowledgeIcon className="h-5 w-5" />}
            />
          ) : null}

          <ul className="space-y-1">
            {entityList.map((entity) => {
              const active = entity.id === entityId;
              return (
                <li key={entity.id}>
                  <button
                    type="button"
                    onClick={() => navigate(`/knowledge/${entity.id}`)}
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
        </div>

        <div className="min-w-0">
          {!entityId ? (
            <EmptyState
              title="选择一个实体"
              description="左侧列表来自本地 Knowledge 库。选中后可查看别名、Claim、关系、时间线与邻域图。"
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
