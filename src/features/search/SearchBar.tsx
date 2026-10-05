import { SearchIcon, SparkIcon } from '@/components/icons';
import { Button } from '@/components/ui/Button';
import { cn } from '@/lib/cn';

export type SearchMode = 'search' | 'ask';

const MODES: { key: SearchMode; label: string; hint: string }[] = [
  { key: 'search', label: '检索', hint: '本地关键词 / 语义，快且免费' },
  { key: 'ask', label: '问答', hint: 'AI 基于知识库作答，给出引用' },
];

/**
 * 搜索输入区（任务书 §34 的组件拆分）。
 *
 * 性能约束（任务书 §28）：**输入过程中绝不触发 IPC**，只有提交才搜索。
 * 这里没有 debounce 也没有 onChange 回调到父层的搜索逻辑，只有本地 state 提升。
 */
export function SearchBar({
  query,
  onQueryChange,
  mode,
  onModeChange,
  onSubmit,
  loading,
  aiEnabled,
}: {
  query: string;
  onQueryChange: (value: string) => void;
  mode: SearchMode;
  onModeChange: (mode: SearchMode) => void;
  onSubmit: () => void;
  loading: boolean;
  aiEnabled: boolean;
}) {
  const askMode = mode === 'ask';

  return (
    <div className="space-y-3">
      <div className="flex gap-2">
        <div className="relative flex-1">
          <SearchIcon className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted" />
          <input
            value={query}
            onChange={(event) => onQueryChange(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                event.preventDefault();
                onSubmit();
              }
            }}
            placeholder={
              askMode
                ? '问一个问题，例如：Rust 的所有权解决了什么问题？'
                : '搜索你的知识……'
            }
            aria-label={askMode ? '提问' : '搜索'}
            className={cn(
              'h-11 w-full rounded-lg border border-line bg-canvas pl-10 pr-3 text-sm text-ink',
              'outline-none transition-colors placeholder:text-muted/70 focus:border-accent/70',
            )}
          />
        </div>
        <Button
          type="button"
          variant="primary"
          size="lg"
          loading={loading}
          onClick={onSubmit}
          disabled={loading || query.trim() === ''}
        >
          {askMode ? <SparkIcon className="h-3.5 w-3.5" /> : null}
          {askMode ? '提问' : '搜索'}
        </Button>
      </div>

      {/* 模式切换：默认检索（快、零成本），问答是显式的增强层而非主体。 */}
      <div className="flex flex-wrap items-center gap-2">
        <div
          role="tablist"
          aria-label="检索模式"
          className="flex rounded-lg border border-line bg-surface p-0.5"
        >
          {MODES.map((item) => (
            <button
              key={item.key}
              type="button"
              role="tab"
              aria-selected={mode === item.key}
              title={item.hint}
              disabled={item.key === 'ask' && !aiEnabled}
              onClick={() => onModeChange(item.key)}
              className={cn(
                'rounded-md px-3 py-1.5 text-xs font-medium transition-colors',
                'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50',
                mode === item.key ? 'bg-accent/10 text-accent' : 'text-muted hover:text-ink',
                item.key === 'ask' && !aiEnabled && 'cursor-not-allowed opacity-50',
              )}
            >
              {item.label}
              {item.key === 'ask' && !aiEnabled ? (
                <span className="ml-1 text-[10px]">未启用</span>
              ) : null}
            </button>
          ))}
        </div>
        <span className="text-meta text-muted">
          {askMode
            ? '答案由模型生成，可能出错——每条结论都应能点开来源核对。'
            : '只查本地索引，不调用模型。'}
        </span>
      </div>
    </div>
  );
}
