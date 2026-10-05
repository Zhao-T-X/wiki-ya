import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from '@/components/ui/Accordion';
import { Badge } from '@/components/ui/Badge';
import { cn } from '@/lib/cn';
import { searchKindLabel } from '@/lib/status';
import type { SearchKind } from '@/types/ipc';

/**
 * 可选的 kind 集合。
 *
 * **不含 `chunk`**：`search_service.rs:82` 是 `SearchHitKind::Chunk => continue`，
 * 切片级检索根本没接上。旧界面一直把它当筛选项展示，用户勾选后必然零结果——
 * 那是界面在提供一个做不到的承诺。这里直接不展示（任务书 §15「不得让用户理解
 * ontology 才能搜索」）。等后端真的支持了切片检索，再加回来。
 */
const SELECTABLE: SearchKind[] = ['claim', 'entity', 'document'];

const ALL: SearchKind[] = [...SELECTABLE];

/** 一级视图：全部 / 知识 / 来源（任务书 §15）。 */
const SCOPES: { key: string; label: string; kinds: SearchKind[] }[] = [
  { key: 'all', label: '全部', kinds: ALL },
  { key: 'knowledge', label: '知识', kinds: ['claim', 'entity'] },
  { key: 'source', label: '来源', kinds: ['document'] },
];

function sameKinds(a: SearchKind[], b: SearchKind[]): boolean {
  if (a.length !== b.length) return false;
  const set = new Set(a);
  return b.every((kind) => set.has(kind));
}

export function SearchFilters({
  kinds,
  onKindsChange,
  semantic,
  onSemanticChange,
  aiEnabled,
  resultCount,
}: {
  kinds: SearchKind[];
  onKindsChange: (kinds: SearchKind[]) => void;
  semantic: boolean;
  onSemanticChange: (value: boolean) => void;
  aiEnabled: boolean;
  resultCount?: number;
}) {
  const activeScope = SCOPES.find((scope) => sameKinds(scope.kinds, kinds))?.key ?? 'custom';

  return (
    <div className="space-y-2">
      {/* 一级：三个粗粒度视图。默认「全部」。 */}
      <div className="flex flex-wrap items-center gap-2">
        <div role="tablist" aria-label="结果范围" className="flex rounded-lg border border-line bg-surface p-0.5">
          {SCOPES.map((scope) => (
            <button
              key={scope.key}
              type="button"
              role="tab"
              aria-selected={activeScope === scope.key}
              onClick={() => onKindsChange([...scope.kinds])}
              className={cn(
                'rounded-md px-3 py-1 text-xs font-medium transition-colors',
                'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50',
                activeScope === scope.key ? 'bg-accent/10 text-accent' : 'text-muted hover:text-ink',
              )}
            >
              {scope.label}
            </button>
          ))}
        </div>
        {resultCount !== undefined ? (
          <span className="text-meta text-muted">{resultCount} 条结果</span>
        ) : null}
      </div>

      {/* 二级：具体 kind 与检索方式收进「高级」，不占主界面。 */}
      <Accordion type="single" collapsible>
        <AccordionItem value="advanced">
          <AccordionTrigger>高级：检索范围与方式</AccordionTrigger>
          <AccordionContent>
            <div className="space-y-3">
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-meta text-muted">类型</span>
                {SELECTABLE.map((kind) => {
                  const active = kinds.includes(kind);
                  return (
                    <button
                      key={kind}
                      type="button"
                      onClick={() =>
                        onKindsChange(
                          active ? kinds.filter((k) => k !== kind) : [...kinds, kind],
                        )
                      }
                      className={cn(
                        'rounded-md border px-2.5 py-1 text-meta font-medium transition-colors',
                        active
                          ? 'border-accent/40 bg-accent/10 text-accent'
                          : 'border-line bg-elevated text-muted hover:text-ink',
                      )}
                    >
                      {searchKindLabel(kind)}
                    </button>
                  );
                })}
              </div>

              <label
                className={cn(
                  'flex items-center gap-2 text-meta',
                  aiEnabled ? 'text-muted' : 'cursor-not-allowed text-muted/50',
                )}
                title={aiEnabled ? '启用语义检索' : '语义检索需要 AI Runtime，当前未启用'}
              >
                <input
                  type="checkbox"
                  checked={semantic}
                  disabled={!aiEnabled}
                  onChange={(event) => onSemanticChange(event.target.checked)}
                  className="h-3.5 w-3.5 accent-accent"
                />
                语义检索
                {!aiEnabled ? <Badge>未启用</Badge> : null}
                <span className="text-muted/70">
                  （按语义相近度而非关键词匹配；当前版本尚未接入检索链路）
                </span>
              </label>
            </div>
          </AccordionContent>
        </AccordionItem>
      </Accordion>
    </div>
  );
}
