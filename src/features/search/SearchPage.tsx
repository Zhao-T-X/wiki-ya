import { useState, type FormEvent } from 'react';
import { Link, useNavigate } from 'react-router-dom';

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
import { formatScore, formatTookMs, newRunId } from '@/lib/format';
import { useAsyncData } from '@/lib/hooks';
import { hitTarget } from '@/lib/links';
import { searchKindLabel } from '@/lib/status';
import { useAgentEvents } from '@/lib/useAgentEvents';
import type { AskResponse, AskSource, SearchHit, SearchKind, SearchResponse } from '@/types/ipc';

const ALL_KINDS: SearchKind[] = ['document', 'chunk', 'claim', 'entity'];

/** 「知识」类命中优先展示；文档/片段作为「来源」排在后面。 */
const KNOWLEDGE_KINDS: SearchKind[] = ['claim', 'entity'];

function sourcePath(source: AskSource): string | null {
  switch (source.kind) {
    case 'entity':
      return `/knowledge/${source.id}`;
    case 'claim':
      return `/claims/${source.id}`;
    case 'document':
      return `/documents/${source.id}`;
    default:
      return null;
  }
}

/**
 * Search / Ask 统一入口（UX 重构）。
 *
 * 用户不需要判断"该搜索还是该问 AI"——只有一个输入框：
 * 默认给出**知识优先**的本地检索结果；AI 可用时，一键让 AI 基于同样的检索给出回答。
 * 内部检索细节（kind 筛选 / 语义开关）收进「高级」。
 */
export function SearchPage() {
  const navigate = useNavigate();
  const appInfo = useAsyncData(() => app_info(), []);
  const aiEnabled = appInfo.data?.aiEnabled ?? false;

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

  useAgentEvents(runId, (event) => {
    if (event.type === 'token_delta') {
      setStreamingAnswer((prev) => prev + event.delta);
    }
  });

  function toggleKind(kind: SearchKind) {
    setKinds((prev) => (prev.includes(kind) ? prev.filter((item) => item !== kind) : [...prev, kind]));
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

  function handleSubmit(event: FormEvent) {
    event.preventDefault();
    const trimmed = query.trim();
    if (trimmed === '') return;
    void runSearch(trimmed);
  }

  async function handleAnswer() {
    const trimmed = query.trim();
    if (!trimmed || answering) return;

    const id = newRunId();
    setRunId(id);
    setStreamingAnswer('');
    setAnswering(true);
    setError(null);
    try {
      setAnswer(await ask({ question: trimmed, role: 'auto', runId: id }));
    } catch (cause: unknown) {
      setError(cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause)));
      setAnswer(null);
    } finally {
      setStreamingAnswer('');
      setRunId(null);
      setAnswering(false);
    }
  }

  const hits = response?.hits ?? [];
  const knowledgeHits = hits.filter((hit) => KNOWLEDGE_KINDS.includes(hit.kind));
  const sourceHits = hits.filter((hit) => !KNOWLEDGE_KINDS.includes(hit.kind));

  return (
    <div>
      <PageHeader
        title="Search"
        subtitle="搜知识，或直接问问题。知识优先于文档；AI 可用时还能基于你的知识库作答。"
      />

      <form onSubmit={handleSubmit} className="space-y-3">
        <div className="flex gap-2">
          <div className="relative flex-1">
            <SearchIcon className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted" />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="搜索或提问，例如：React 现在是什么版本？"
              className="h-11 w-full rounded-lg border border-line bg-canvas pl-10 pr-3 text-sm text-ink outline-none transition-colors placeholder:text-muted/70 focus:border-accent/70"
            />
          </div>
          <Button type="submit" variant="primary" loading={loading} disabled={loading}>
            搜索
          </Button>
          {aiEnabled && query.trim() !== '' ? (
            <Button type="button" loading={answering} disabled={answering} onClick={handleAnswer}>
              <SparkIcon className="h-3.5 w-3.5" />
              让 AI 回答
            </Button>
          ) : null}
        </div>

        <details className="rounded-lg border border-line bg-surface px-3 py-2">
          <summary className="cursor-pointer text-[11px] font-medium text-muted">高级</summary>
          <div className="mt-2 flex flex-wrap items-center gap-2">
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
        </details>
      </form>

      {error ? <ErrorNotice error={error} className="mt-4" /> : null}

      {/* AI 回答优先展示在检索结果之上 */}
      {answering || answer ? (
        <Card className="mt-4 p-5">
          {answering ? (
            streamingAnswer ? (
              <>
                <h3 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-muted">
                  Answer（生成中…）
                </h3>
                <div className="whitespace-pre-wrap break-words text-sm leading-relaxed text-ink/90">
                  {streamingAnswer}
                </div>
              </>
            ) : (
              <div className="flex items-center gap-2 text-xs text-muted">
                <Spinner className="h-3.5 w-3.5" />
                正在基于你的知识库回答…
              </div>
            )
          ) : answer && !answer.enabled ? (
            <div>
              <p className="text-sm font-medium text-ink">当前无法回答</p>
              <p className="mt-1 text-xs leading-relaxed text-muted">
                {answer.note ?? 'AI 未启用或问答链路暂不可用。请确认已配置 WIKIYA_API_KEY。'}
              </p>
            </div>
          ) : answer ? (
            <>
              <h3 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-muted">Answer</h3>
              <div className="whitespace-pre-wrap break-words text-sm leading-relaxed text-ink/90">
                {answer.answer}
              </div>

              {answer.sources.length > 0 ? (
                <ul className="mt-4 space-y-2 border-t border-line pt-3">
                  {answer.sources.map((source) => {
                    const path = sourcePath(source);
                    return (
                      <li key={`${source.kind}-${source.id}-${source.index}`} className="text-xs">
                        <span className="font-mono text-muted">[{source.index}]</span>{' '}
                        {path ? (
                          <Link to={path} className="text-accent hover:underline">
                            {source.title}
                          </Link>
                        ) : (
                          <span className="text-ink">{source.title}</span>
                        )}
                        <p className="mt-0.5 whitespace-pre-wrap break-words text-[11px] leading-relaxed text-muted">
                          {source.snippet}
                        </p>
                      </li>
                    );
                  })}
                </ul>
              ) : null}

              {/* 内部统计默认收起 */}
              {answer.contextStats ? (
                <details className="mt-3 border-t border-line pt-2">
                  <summary className="cursor-pointer text-[10px] uppercase tracking-wider text-muted">
                    上下文统计
                  </summary>
                  <p className="mt-2 font-mono text-[10px] text-muted">
                    tokens {answer.contextStats.loadedTokens}/{answer.contextStats.totalTokens} · items{' '}
                    {answer.contextStats.itemCount} · 压缩 {answer.contextStats.compressionRatio.toFixed(2)}x
                    {answer.contextStats.truncated ? ' · 已截断' : ''}
                  </p>
                </details>
              ) : null}
            </>
          ) : null}
        </Card>
      ) : null}

      {response ? (
        <div className="mt-4 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-muted">
          <span>{response.total} 条结果</span>
          <span className="text-line">·</span>
          <span>用时 {formatTookMs(response.tookMs)}</span>
          <span className="text-line">·</span>
          <span>{response.method}</span>
        </div>
      ) : null}

      {response?.notice ? (
        <p className="mt-2 rounded-md border border-line bg-elevated/60 px-3 py-2 text-[11px] leading-relaxed text-muted">
          {response.notice}
        </p>
      ) : null}

      {submitted && !loading && hits.length === 0 && !error ? (
        <EmptyState
          className="mt-4"
          title="没有匹配结果"
          description="换一个关键字，或放宽筛选条件。"
          icon={<SearchIcon className="h-5 w-5" />}
        />
      ) : null}

      {loading && !response ? (
        <div className="mt-4 flex items-center gap-2 text-xs text-muted">
          <Spinner className="h-3.5 w-3.5" />
          检索中…
        </div>
      ) : null}

      {knowledgeHits.length > 0 ? (
        <section className="mt-6">
          <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-muted">知识</h3>
          <div className="space-y-2">
            {knowledgeHits.map((hit) => (
              <HitCard key={`${hit.kind}-${hit.id}`} hit={hit} onOpen={navigate} />
            ))}
          </div>
        </section>
      ) : null}

      {sourceHits.length > 0 ? (
        <section className="mt-6">
          <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-muted">
            {knowledgeHits.length > 0 ? '来源文档' : '结果'}
          </h3>
          <div className="space-y-2">
            {sourceHits.map((hit) => (
              <HitCard key={`${hit.kind}-${hit.id}`} hit={hit} onOpen={navigate} />
            ))}
          </div>
        </section>
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
