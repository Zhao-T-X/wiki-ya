import { useState, type FormEvent } from 'react';
import { useNavigate } from 'react-router-dom';

import { ErrorNotice } from '@/components/ErrorNotice';
import { PageHeader } from '@/components/PageHeader';
import { SearchIcon } from '@/components/icons';
import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { EmptyState } from '@/components/ui/EmptyState';
import { Spinner } from '@/components/ui/Spinner';
import { app_info, search, WikiError } from '@/lib/api';
import { cn } from '@/lib/cn';
import { formatScore, formatTookMs } from '@/lib/format';
import { useAsyncData } from '@/lib/hooks';
import { hitTarget } from '@/lib/links';
import { searchKindLabel } from '@/lib/status';
import type { SearchKind, SearchResponse } from '@/types/ipc';

const ALL_KINDS: SearchKind[] = ['document', 'chunk', 'claim', 'entity'];

export function SearchPage() {
  const navigate = useNavigate();
  const appInfo = useAsyncData(() => app_info(), []);

  const [query, setQuery] = useState('');
  const [kinds, setKinds] = useState<SearchKind[]>([...ALL_KINDS]);
  const [semantic, setSemantic] = useState(false);
  const [response, setResponse] = useState<SearchResponse | null>(null);
  const [error, setError] = useState<WikiError | null>(null);
  const [loading, setLoading] = useState(false);
  const [submitted, setSubmitted] = useState(false);

  const aiEnabled = appInfo.data?.aiEnabled ?? false;

  function toggleKind(kind: SearchKind) {
    setKinds((prev) => (prev.includes(kind) ? prev.filter((item) => item !== kind) : [...prev, kind]));
  }

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    const trimmed = query.trim();
    if (trimmed === '') return;

    setLoading(true);
    setError(null);
    setSubmitted(true);
    // 清空上一次结果，避免出错时「新旧结果 + 错误提示」同时出现。
    setResponse(null);

    try {
      const result = await search({
        query: trimmed,
        limit: 30,
        // Phase 6 前 semantic 恒为降级；未启用 AI 时前端强制传 false。
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

  const hits = response?.hits ?? [];

  return (
    <div>
      <PageHeader
        title="Search"
        subtitle="统一检索：文档 / 片段 / Claim / 实体。当前为本地词法检索（FTS5 trigram）。"
      />

      <form onSubmit={handleSubmit} className="space-y-3">
        <div className="flex gap-2">
          <div className="relative flex-1">
            <SearchIcon className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted" />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search everything…"
              className="h-10 w-full rounded-lg border border-line bg-canvas pl-10 pr-3 text-sm text-ink outline-none transition-colors placeholder:text-muted/70 focus:border-accent/70"
            />
          </div>
          <Button type="submit" variant="primary" loading={loading} disabled={loading}>
            搜索
          </Button>
        </div>

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
            title={aiEnabled ? '启用语义检索' : '语义检索需要 AI Runtime（Phase 6），当前未启用'}
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
      </form>

      {error ? <ErrorNotice error={error} className="mt-4" /> : null}

      {response ? (
        <div className="mt-4 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-muted">
          <Badge tone="accent">method: {response.method}</Badge>
          <span>{response.total} 条结果</span>
          <span className="text-line">·</span>
          <span>用时 {formatTookMs(response.tookMs)}</span>
          {!aiEnabled ? <span className="text-muted/70">（语义检索未启用，已降级为词法检索）</span> : null}
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
          description="换一个关键字，或放宽 kind 筛选。中文检索依赖 FTS5 trigram，建议使用连续子串。"
          icon={<SearchIcon className="h-5 w-5" />}
        />
      ) : null}

      {loading && !response ? (
        <div className="mt-4 flex items-center gap-2 text-xs text-muted">
          <Spinner className="h-3.5 w-3.5" />
          检索中…
        </div>
      ) : null}

      <div className="mt-3 space-y-2">
        {hits.map((hit) => {
          const target = hitTarget(hit);
          return (
            <Card key={`${hit.kind}-${hit.id}`} className="p-3.5">
              <div className="flex items-start justify-between gap-3">
                <button
                  type="button"
                  disabled={!target}
                  onClick={() => target && navigate(target)}
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
                <span className="text-[10px] text-muted/70">为什么匹配：命中字段</span>
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
        })}
      </div>
    </div>
  );
}
