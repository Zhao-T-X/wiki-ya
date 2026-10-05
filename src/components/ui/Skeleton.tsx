import { cn } from '@/lib/cn';

/**
 * 骨架屏。
 *
 * 任务书 §26 只允许「结果加载淡入」这一类动画，骨架屏正好对应：它不是装饰动画，
 * 而是**如实表达"正在取数据"**。此前 Search / Review 加载态只有一句「检索中…」，
 * 布局会先空后跳。
 */
export function Skeleton({ className }: { className?: string }) {
  return <div className={cn('animate-pulse rounded-md bg-elevated', className)} />;
}

/** 常用骨架组合：标题 + 两行正文 + 标签行。 */
export function SkeletonCard({ lines = 2, className }: { lines?: number; className?: string }) {
  return (
    <div className={cn('space-y-2 rounded-xl border border-line p-4', className)}>
      <Skeleton className="h-4 w-2/5" />
      {Array.from({ length: lines }, (_, i) => (
        <Skeleton key={i} className={cn('h-3', i % 2 === 0 ? 'w-full' : 'w-4/5')} />
      ))}
      <div className="flex gap-1.5 pt-1">
        <Skeleton className="h-3.5 w-12" />
        <Skeleton className="h-3.5 w-16" />
      </div>
    </div>
  );
}
