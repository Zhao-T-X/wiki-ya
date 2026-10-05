import { ReviewIcon } from '@/components/icons';
import { ReviewQueueItem } from '@/features/review/ReviewQueueItem';
import { Button } from '@/components/ui/Button';
import { EmptyState } from '@/components/ui/EmptyState';
import { SkeletonCard } from '@/components/ui/Skeleton';
import { cn } from '@/lib/cn';
import { relationshipLabel } from '@/lib/status';
import type { ReviewItem } from '@/types/ipc';

/**
 * 待审队列（任务书 §20、§25）。
 *
 * 筛选项**固定**为受控词表里的六种关系（任务书 §25 明确禁止「从 all 数据动态
 * 生成」——那样会让筛选项随数据变化，用户无法形成稳定预期）。
 */
const FILTERS: { key: string | null; label: string }[] = [
  { key: null, label: '全部' },
  { key: 'contradicts', label: relationshipLabel('contradicts') },
  { key: 'supersedes', label: relationshipLabel('supersedes') },
  { key: 'supplements', label: relationshipLabel('supplements') },
  { key: 'duplicate', label: relationshipLabel('duplicate') },
  { key: 'coexists', label: relationshipLabel('coexists') },
];

export function ReviewQueue({
  items,
  loading,
  filter,
  onFilterChange,
  selectedId,
  onSelect,
}: {
  items: ReviewItem[];
  loading: boolean;
  filter: string | null;
  onFilterChange: (relationship: string | null) => void;
  selectedId: string | null;
  onSelect: (id: string) => void;
}) {
  return (
    <div className="flex h-full flex-col gap-3">
      <div className="flex flex-wrap items-center gap-1.5">
        {FILTERS.map((item) => (
          <Button
            key={item.key ?? 'all'}
            size="xs"
            variant={filter === item.key ? 'secondary' : 'ghost'}
            onClick={() => onFilterChange(item.key)}
            className={cn(filter === item.key && 'text-accent')}
          >
            {item.label}
          </Button>
        ))}
      </div>

      <div className="min-h-0 flex-1 space-y-1 overflow-y-auto pr-1">
        {loading && items.length === 0 ? (
          <>
            <SkeletonCard lines={2} />
            <SkeletonCard lines={2} />
            <SkeletonCard lines={2} />
          </>
        ) : null}

        {!loading && items.length === 0 ? (
          <EmptyState
            title="没有待决策的变更"
            description="当新知识与已有知识产生重复、补充或冲突时，会出现在这里。"
            icon={<ReviewIcon className="h-5 w-5" />}
          />
        ) : null}

        {items.map((item) => (
          <ReviewQueueItem
            key={item.relation.id}
            item={item}
            selected={item.relation.id === selectedId}
            onSelect={onSelect}
          />
        ))}
      </div>
    </div>
  );
}
