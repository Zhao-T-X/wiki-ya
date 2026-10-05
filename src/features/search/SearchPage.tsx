import { useState, type FormEvent } from 'react';
import { useNavigate } from 'react-router-dom';

import { Collapse } from '@/components/agent/Collapse';
import { Markdown } from '@/components/agent/Markdown';
import { SourceList } from '@/components/agent/SourceCard';
import { TokenLedger } from '@/components/agent/TokenLedger';
import { ErrorNotice } from '@/components/ErrorNotice';
import { PageHeader } from '@/components/PageHeader';
import { SearchIcon, SparkIcon } from '@/components/icons';
import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { EmptyState } from '@/components/ui/EmptyState';
import { Spinner } from '@/components/ui/Spinner';
import { app_info, ask, search, WikiError } from '@/lib/api';
import { cn } from '@/lib/cn';
import { formatCompressionRatio, formatScore, formatTookMs, newRunId } from '@/lib/format';
import { useAsyncData } from '@/lib/hooks';
import { hitTarget } from '@/lib/links';
import { searchKindLabel } from '@/lib/status';
import { useRunEvents } from '@/lib/useRunEvents';
import type { AskResponse, SearchHit, SearchKind, SearchResponse } from '@/types/ipc';

const ALL_KINDS: SearchKind[] = ['document', 'chunk', 'claim', 'entity'];

/** 「知识」类命中优先展示；文档/片段作为「来源」排在后面。 */
const KNOWLEDGE_KINDS: SearchKind[] = ['claim', 'entity'];

/**
 * 两种模式。
 *
 * - `search`：纯本地检索。快、零成本、结果可扫读，是默认。
 * - `ask`：让模型基于知识库作答。慢、要 token，但给出带引用的结论。
 *
 * 此前两者是「检索 + 一个孤立的『让 AI 回答』按钮」，且 `ask` 内部自己检索，
 * 与 search 的结果各调一次 API、互不相干，页面上是并列的两个孤岛。现在至少
 * 在**呈现**上建立了关系：答案里的 [n] 角标能点到底部证据。
 */
type Mode = 'search' | 'ask';

const MODES: { key: Mode; label: string; hint: string }[] = [
  { key: 'search', label: '检索', hint: '本地关键词 / 语义，快且免费' },
  { key: 'ask', label: '问答', hint: 'AI 基于知识库作答，给出引用' },
];

export function SearchPage() {
  const navigate = useNavigate();
  const appInfo = useAsyncData(() => app_info(), []);
  const aiEnabled = appInfo.data?.aiEnabled ?? false;

  const [mode, setMode] = useState<Mode>('search');
  const [query, setQuery] = useState('');
  const [kinds, setKinds] = useState<SearchKind[]>([...ALL_KINDS]);
  const [semantic, setSemantic] = useState(false);

  const [response, setResponse] = useState<SearchResponse | null>(null);
  const [error, setError] = useState<WikiError | null>(null);
  const [loading, setLoading] = useState(false);
  const [submitted, setSubmitted] = useState(false);

  const [answer, setAnswer] = useState<AskResponse | null>(null);
  const [answering, setAnswering] = useState(false);
  const [runId, setRunId] = useState<string | null>(null);
  const [streamingAnswer, setStreamingAnswer] = useState('');
  /** 正文里点了 [n] 之后被点亮的来源序号。 */
  const [activeSource, setActiveSource] = useState<number | null>(null);

  useRunEvents(runId, (event) => {
    if (event.kind === 'tokenDelta') {
      setStreamingAnswer((prev) => prev + event.delta);
    }
  });

  function toggleKind(kind: SearchKind) {
    setKinds((prev) => (prev.includes(kind) ? prev.filter((item) => item !== kind) : [...prev, kind]));
  }

  /** 点了正文角标：滚动到对应证据并点亮它。 */
  function jumpToSource(index: number) {
    setActiveSource(index);
    const node = document.getElementById(`source-${index}`);
    node?.scrollIntoView({ behavior: 'smooth', block: 'center' });
  }

  function switchMode(next: Mode) {
    setMode(next);
    setActiveSource(null);
    // 两种模式的数据互不相干（ask 自己检索），留着上一份只会让人困惑。
    if (next === 'ask') {
      setResponse(null);
      setSubmitted(false);
    } else {
      setAnswer(null);
      setStreamingAnswer('');
    }
  }

  async function runSearch(rawQuery: string) {
    setLoading(true);
    setError(null);
    setSubmitted(true);
    setResponse(null);
    setAnswer(null);

    try {
      const result = await search({
        query: rawQuery,
        limit: 30,
        // 未启用 AI 时语义检索必然降级，前端直接传 false，避免产生误导性提示。
        semantic: aiEnabled ? semantic : false,
        kinds: kinds.length === 0 ? undefined : kinds,
      });
      setResponse(result);
    } catch (cause: unknown) {
      setError(cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause)));
    } finally {
      setLoading(false);
    }
  }

  async function handleAnswer(question: string) {
    if (question === '' || answering) return;

    const id = newRunId();
    setRunId(id);
    setStreamingAnswer('');
    setAnswering(true);
    setError(null);
    try {
      setAnswer(await ask({ question, role: 'auto', runId: id }));
    } catch (cause: unknown) {
      setError(cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause)));
      setAnswer(null);
    } finally {
      setStreamingAnswer('');
      setRunId(null);
      setAnswering(false);
    }
  }

  function handleSubmit(event: FormEvent) {
    event.preventDefault();
    const trimmed = query.trim();
    if (trimmed === '') return;
    if (mode === 'ask') void handleAnswer(trimmed);
    else void runSearch(trimmed);
  }

  const hits = response?.hits ?? [];
  const knowledgeHits = hits.filter((hit) => KNOWLEDGE_KINDS.includes(hit.kind));
  const sourceHits = hits.filter((hit) => !KNOWLEDGE_KINDS.includes(hit.kind));
  const showAnswerPanel = mode === 'ask' && (answering || answer !== null);
  const showSearchPanel = mode === 'search' && submitted;

  return (
    <div>
      <PageHeader
        title="Search"
        subtitle="搜知识库，或让 AI 基于你的知识库作答。答案里的 [n] 可以点开对应来源。"
      />

      <form onSubmit={handleSubmit} className="space-y-3">
        <div className="flex gap-2">
          <div className="relative flex-1">
            <SearchIcon className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted" />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={
                mode === 'ask'
                  ? '问一个问题，例如：Rust 的所有权解决了什么问题？'
                  : '搜索关键词，例如：所有权'
              }
              className="h-11 w-full rounded-lg border border-line bg-canvas pl-10 pr-3 text-sm text-ink outline-none transition-colors placeholder:text-muted/70 focus:border-accent/70"
            />
          </div>
          <Button type="submit" variant="primary" loading={loading || answering}>
            {mode === 'ask' ? '提问' : '搜索'}
          </Button>
        </div>

        {/* 模式切换：默认检索（快、零成本），问答显式选择。 */}
        <div className="flex flex-wrap items-center gap-2">
          <div className="flex rounded-lg border border-line bg-surface p-0.5">
            {MODES.map((item) => (
              <button
                key={item.key}
                type="button"
                title={item.hint}
                onClick={() => switchMode(item.key)}
                disabled={item.key === 'ask' && !aiEnabled}
                className={cn(
                  'rounded-md px-3 py-1.5 text-[11px] font-medium transition-colors',
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
          <span className="text-[11px] text-muted">
            {mode === 'ask'
              ? '答案由模型生成，可能出错——每条结论都应能点开来源核对。'
              : '只查本地索引，不调用模型。'}
          </span>
        </div>

        {mode === 'search' ? (
          <Collapse title="筛选与检索方式" hint={`${kinds.length}/${ALL_KINDS.length} 类`}>
            <div className="flex flex-wrap items-center gap-2">
              {ALL_KINDS.map((kind) => {
                const active = kinds.includes(kind);
                return (
                  <button
                    key={kind}
                    type="button"
                    onClick={() => toggleKind(kind)}
                    className={cn(
                      'rounded-md border px-2.5 py-1 text-[11px] font-medium transition-colors',
                      active
                        ? 'border-accent/40 bg-accent/10 text-accent'
                        : 'border-line bg-elevated text-muted hover:text-ink',
                    )}
                  >
                    {searchKindLabel(kind)}
                  </button>
                );
              })}

              <span className="mx-1 h-4 w-px bg-line" />

              <label
                className={cn(
                  'flex items-center gap-2 text-[11px]',
                  aiEnabled ? 'text-muted' : 'cursor-not-allowed text-muted/50',
                )}
                title={aiEnabled ? '启用语义检索' : '语义检索需要 AI Runtime，当前未启用'}
              >
                <input
                  type="checkbox"
                  checked={semantic}
                  disabled={!aiEnabled}
                  onChange={(event) => setSemantic(event.target.checked)}
                  className="h-3.5 w-3.5 accent-accent"
                />
                语义检索
                {!aiEnabled ? <Badge>未启用</Badge> : null}
              </label>
            </div>
          </Collapse>
        ) : null}
      </form>

      {error ? <ErrorNotice error={error} className="mt-4" /> : null}

      {/* ---------------- 问答模式：答案 + 证据链 ---------------- */}
      {showAnswerPanel ? (
        <div className="mt-4 space-y-3">
          <Card className="p-5">
            <div className="mb-3 flex items-center justify-between gap-2">
              <h3 className="text-[11px] font-semibold uppercase tracking-wider text-muted">
                回答
              </h3>
              {answering ? (
                <Badge tone="accent">
                  <Spinner className="mr-1 h-3 w-3" />
                  生成中
                </Badge>
              ) : null}
            </div>

            {answering && streamingAnswer === '' ? (
              <div className="flex items-center gap-2 text-xs text-muted">
                <Spinner className="h-3.5 w-3.5" />
                正在检索知识库并组织答案…
              </div>
            ) : null}

            {streamingAnswer ? (
              <Markdown text={streamingAnswer} onCitation={jumpToSource} className="animate-pulse" />
            ) : null}

            {answer && !answer.enabled ? (
              <div>
                <p className="text-sm font-medium text-ink">当前无法回答</p>
                <p className="mt-1 text-xs leading-relaxed text-muted">
                  {answer.note ?? 'AI 未启用或问答链路暂不可用。请在设置里配置 API Key。'}
                </p>
              </div>
            ) : null}

            {answer && answer.enabled ? (
              <>
                <Markdown text={answer.answer} onCitation={jumpToSource} />
                {answer.note ? (
                  <p className="mt-3 text-[11px] leading-relaxed text-warn">{answer.note}</p>
                ) : null}
              </>
            ) : null}
          </Card>

          {/* 证据区：答案里每条 [n] 都对应这里一张卡片 */}
          {answer && answer.sources.length > 0 ? (
            <Card className="p-5">
              <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-muted">
                来源（{answer.sources.length}）
              </h3>
              <SourceList sources={answer.sources} activeIndex={activeSource} />
            </Card>
          ) : null}

          {/* 内部细节收进高级：Token / Embedding / RRF 不该占主界面
              （docs/约束.md §38：普通用户不应需要理解这些）。 */}
          {answer ? (
            <Collapse
              title="运行细节"
              hint={answer.agentRunId ? answer.agentRunId.slice(0, 8) : undefined}
            >
              <div className="space-y-2">
                <TokenLedger usage={answer.usage} costUsd={answer.costUsd} />
                {answer.contextStats ? (
                  <p className="font-mono text-[10px] leading-relaxed text-muted">
                    上下文 {answer.contextStats.loadedTokens}/{answer.contextStats.totalTokens}{' '}
                    tokens · {answer.contextStats.itemCount} 条 · 压缩比{' '}
                    {formatCompressionRatio(answer.contextStats.compressionRatio)}
                    {answer.contextStats.truncated ? ' · 已截断' : ''}
                  </p>
                ) : null}
              </div>
            </Collapse>
          ) : null}
        </div>
      ) : null}

      {mode === 'ask' && !showAnswerPanel ? (
        <EmptyState
          className="mt-4"
          title="问点什么"
          description="回答会基于你的知识库，并给出可点开的来源。"
          icon={<SparkIcon className="h-5 w-5" />}
        />
      ) : null}

      {/* ---------------- 检索模式：命中列表 ---------------- */}
      {showSearchPanel ? (
        <div className="mt-4 space-y-3">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-muted">
            <span>{response?.total ?? 0} 条结果</span>
            {response ? (
              <>
                <span className="text-line">·</span>
                <span>用时 {formatTookMs(response.tookMs)}</span>
                <span className="text-line">·</span>
                <span>检索方式 {response.method}</span>
              </>
            ) : null}
          </div>

          {response?.notice ? (
            <p className="rounded-md border border-line bg-elevated/60 px-3 py-2 text-[11px] leading-relaxed text-muted">
              {response.notice}
            </p>
          ) : null}

          {loading ? (
            <div className="flex items-center gap-2 text-xs text-muted">
              <Spinner className="h-3.5 w-3.5" />
              检索中…
            </div>
          ) : null}

          {!loading && hits.length === 0 ? (
            <EmptyState
              title="没有匹配结果"
              description="换一个关键字，或放宽筛选条件。"
              icon={<SearchIcon className="h-5 w-5" />}
            />
          ) : null}

          {knowledgeHits.length > 0 ? (
            <section>
              <h3 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-muted">
                知识
              </h3>
              <div className="space-y-2">
                {knowledgeHits.map((hit) => (
                  <HitCard key={hit.id} hit={hit} onOpen={navigate} />
                ))}
              </div>
            </section>
          ) : null}

          {sourceHits.length > 0 ? (
            <section>
              <h3 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-muted">
                {knowledgeHits.length > 0 ? '来源文档' : '结果'}
              </h3>
              <div className="space-y-2">
                {sourceHits.map((hit) => (
                  <HitCard key={hit.id} hit={hit} onOpen={navigate} />
                ))}
              </div>
            </section>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

function HitCard({ hit, onOpen }: { hit: SearchHit; onOpen: (path: string) => void }) {
  const target = hitTarget(hit);
  return (
    <Card className="p-3.5">
      <div className="flex items-start justify-between gap-3">
        <button
          type="button"
          disabled={!target}
          onClick={() => target && onOpen(target)}
          className="min-w-0 flex-1 text-left disabled:cursor-default"
        >
          <div className="flex items-center gap-2">
            <Badge tone="accent">{searchKindLabel(hit.kind)}</Badge>
            <span className="truncate text-sm text-ink">{hit.title}</span>
          </div>
          <p className="mt-1 text-xs leading-relaxed text-muted">{hit.snippet}</p>
        </button>
        <div className="shrink-0 text-right">
          <p className="font-mono text-[11px] text-ink/80">{formatScore(hit.score)}</p>
          <p className="mt-0.5 font-mono text-[10px] text-muted">{hit.method}</p>
        </div>
      </div>

      <div className="mt-2 flex flex-wrap items-center gap-x-1.5 gap-y-1 border-t border-line pt-2">
        <span className="text-[10px] text-muted/70">命中字段</span>
        {hit.matchedIn.length === 0 ? (
          <span className="text-[10px] text-muted/70">（未提供）</span>
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
      </div>
    </Card>
  );
}
