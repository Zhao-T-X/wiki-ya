import { MarkdownContent } from '@/components/content/MarkdownContent';
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from '@/components/ui/Accordion';
import { Badge } from '@/components/ui/Badge';
import { formatScore } from '@/lib/format';
import { hitTarget } from '@/lib/links';
import { searchKindLabel } from '@/lib/status';
import type { SearchHit } from '@/types/ipc';

/**
 * 单条搜索结果（任务书 §13）。
 *
 * 与改造前的关键差异：**技术元数据默认不显示**。
 *
 * 此前 `score` / `method` / `matchedIn` 三块直接摊在卡片上——而 `score` 是 RRF
 * 融合分（`search_service.rs:132` 把 RRF 分 ×1000 保留三位），`method` 是
 * `lexical`/`semantic` 字面量。这些是 Level 3 技术信息，任务书 §5.2 要求默认
 * 不显示；普通用户看到「检索方式 lexical」得不到任何认知，只会觉得界面粗糙。
 *
 * 现在它们收进「为什么相关 ▾」，默认折叠。
 */
export function SearchResultCard({
  hit,
  onOpen,
}: {
  hit: SearchHit;
  onOpen: (path: string) => void;
}) {
  const target = hitTarget(hit);
  const knowledge = hit.kind === 'claim' || hit.kind === 'entity';

  return (
    <div className="px-4 py-3 transition-colors hover:bg-elevated/50">
      <div className="flex items-start justify-between gap-3">
        <button
          type="button"
          disabled={!target}
          onClick={() => target && onOpen(target)}
          className="min-w-0 flex-1 text-left disabled:cursor-default"
        >
          <div className="flex flex-wrap items-center gap-2">
            <Badge tone={knowledge ? 'accent' : 'neutral'} variant="text">
              {searchKindLabel(hit.kind)}
            </Badge>
            <span className="truncate text-[15px] font-medium leading-snug text-ink">{hit.title}</span>
          </div>
        </button>
      </div>

      {/* 摘要：compact 模式，块边界结构化摘要，避免一条结果渲染整篇文档。 */}
      <MarkdownContent mode="compact" className="mt-1.5 text-secondary">
        {hit.snippet}
      </MarkdownContent>

      <Accordion type="single" collapsible className="mt-1">
        <AccordionItem value="why">
          <AccordionTrigger>为什么相关</AccordionTrigger>
          <AccordionContent>
            <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
              <dt className="text-muted">检索得分</dt>
              <dd className="font-mono text-ink/80">{formatScore(hit.score)}</dd>

              <dt className="text-muted">检索方式</dt>
              <dd className="font-mono text-ink/80">{hit.method}</dd>

              <dt className="text-muted">命中字段</dt>
              <dd className="flex flex-wrap gap-1">
                {hit.matchedIn.length === 0 ? (
                  <span className="text-muted/70">（未提供）</span>
                ) : (
                  hit.matchedIn.map((field) => (
                    <span
                      key={field}
                      className="rounded border border-line bg-canvas px-1.5 py-0.5 font-mono text-[10px] text-muted"
                    >
                      {field}
                    </span>
                  ))
                )}
              </dd>
            </dl>
            <p className="mt-2 text-[10px] leading-relaxed text-muted/70">
              得分是融合排序用的内部数值，不代表「答案正确率」。
            </p>
          </AccordionContent>
        </AccordionItem>
      </Accordion>
    </div>
  );
}

/** 供列表层复用的「是否知识类」判定。 */
export function isKnowledgeHit(hit: SearchHit): boolean {
  return hit.kind === 'claim' || hit.kind === 'entity';
}
