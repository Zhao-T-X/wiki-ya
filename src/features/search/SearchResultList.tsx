import { SearchIcon } from '@/components/icons';
import { SearchResultCard, isKnowledgeHit } from '@/features/search/SearchResultCard';
import { Card } from '@/components/ui/Card';
import { EmptyState } from '@/components/ui/EmptyState';
import { Separator } from '@/components/ui/Separator';
import { SkeletonCard } from '@/components/ui/Skeleton';
import { formatTookMs } from '@/lib/format';
import type { SearchResponse } from '@/types/ipc';

/**
 * 检索结果列表（任务书 §34）。
 *
 * 结构：**知识优先**——命中的 Claim / Entity 在前，文档作为「来源」排在后面。
 * 检索元信息（条数 / 用时 / 方式）压到一行，等级远低于结果本身。
 */
export function SearchResultList({
  response,
  loading,
  submitted,
  error,
  onOpen,
}: {
  response: SearchResponse | null;
  loading: boolean;
  submitted: boolean;
  error: string | null;
  onOpen: (path: string) => void;
}) {
  if (error) {
    return (
      <Card tone="quiet" className="border-danger/30 p-4 text-sm text-danger">
        {error}
      </Card>
    );
  }

  if (loading) {
    return (
      <div className="space-y-2">
        <SkeletonCard lines={2} />
        <SkeletonCard lines={3} />
        <SkeletonCard lines={2} />
      </div>
    );
  }

  if (!submitted) {
    return (
      <EmptyState
        title="搜索你的知识"
        description="输入关键词，从文档与知识卡片里找到答案。知识卡片排在文档前面。"
        icon={<SearchIcon className="h-5 w-5" />}
      />
    );
  }

  const hits = response?.hits ?? [];
  if (hits.length === 0) {
    return (
      <EmptyState
        title="没有匹配结果"
        description="换一个关键字，或到「高级」里放宽类型范围。"
        icon={<SearchIcon className="h-5 w-5" />}
      />
    );
  }

  const knowledge = hits.filter(isKnowledgeHit);
  const sources = hits.filter((hit) => !isKnowledgeHit(hit));

  return (
    <div className="space-y-4">
      {/* 检索元信息：Level 3，压成一行灰字。 */}
      <p className="text-meta text-muted">
        {response?.total ?? hits.length} 条 · 用时 {formatTookMs(response?.tookMs ?? 0)}
        {response?.method ? ` · ${response.method}` : ''}
      </p>

      {response?.notice ? (
        <p className="rounded-md border border-line bg-elevated/60 px-3 py-2 text-meta leading-relaxed text-muted">
          {response.notice}
        </p>
      ) : null}

      {knowledge.length > 0 ? (
        <section className="space-y-2">
          <h2 className="text-sm font-semibold text-ink">知识</h2>
          <div className="divide-y divide-line/70 overflow-hidden rounded-lg border border-line">
            {knowledge.map((hit) => (
              <SearchResultCard key={`${hit.kind}-${hit.id}`} hit={hit} onOpen={onOpen} />
            ))}
          </div>
        </section>
      ) : null}

      {knowledge.length > 0 && sources.length > 0 ? <Separator /> : null}

      {sources.length > 0 ? (
        <section className="space-y-2">
          <h2 className="text-sm font-semibold text-ink">
            {knowledge.length > 0 ? '来源文档' : '结果'}
          </h2>
          <div className="divide-y divide-line/70 overflow-hidden rounded-lg border border-line">
            {sources.map((hit) => (
              <SearchResultCard key={`${hit.kind}-${hit.id}`} hit={hit} onOpen={onOpen} />
            ))}
          </div>
        </section>
      ) : null}
    </div>
  );
}
