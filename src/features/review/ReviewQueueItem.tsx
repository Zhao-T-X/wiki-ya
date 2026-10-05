import { Badge } from '@/components/ui/Badge';
import { cn } from '@/lib/cn';
import { relationshipLabel, relationshipTone } from '@/lib/status';
import type { ReviewItem } from '@/types/ipc';

/**
 * 队列单项（任务书 §20）。
 *
 * **只显示三样**：关系类型、双方文本。这是有意的克制——此前每张卡片都把
 * 「改了什么 / 为什么 / 证据 / 影响 / 优先级 / 置信度 / 建议动作」全摊开，
 * 一屏放不下几条，用户被迫上下滚动才能看完一项。
 *
 * 那些内容全部搬进右栏的决策面板（ReviewDecisionPanel），因为它们服务于
 * **决策**，而队列只服务于**选择**。
 */
export function ReviewQueueItem({
  item,
  selected,
  onSelect,
}: {
  item: ReviewItem;
  selected: boolean;
  onSelect: (id: string) => void;
}) {
  const { relation } = item;

  return (
    <button
      type="button"
      onClick={() => onSelect(relation.id)}
      aria-current={selected}
      className={cn(
        'w-full rounded-lg border px-3 py-2.5 text-left transition-colors',
        'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50',
        selected
          ? 'border-accent/50 bg-accent/5'
          : 'border-transparent hover:border-line hover:bg-elevated/60',
      )}
    >
      <div className="flex items-center gap-2">
        <Badge tone={relationshipTone(relation.relationship)} variant="text">
          {relationshipLabel(relation.relationship)}
        </Badge>
      </div>

      <p className="mt-1.5 line-clamp-2 text-secondary leading-relaxed text-ink/90">
        {relation.sourceText}
      </p>
      <p className="mt-0.5 line-clamp-2 text-secondary leading-relaxed text-muted">
        {relation.targetText}
      </p>
    </button>
  );
}
