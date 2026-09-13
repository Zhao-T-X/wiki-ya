import { FormEvent, useState } from 'react';
import { Link } from 'react-router-dom';

import { ErrorNotice } from '@/components/ErrorNotice';
import { AskIcon, SparkIcon } from '@/components/icons';
import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { Input } from '@/components/ui/Input';
import { Spinner } from '@/components/ui/Spinner';
import { PageHeader } from '@/components/PageHeader';
import { app_info, ask, WikiError } from '@/lib/api';
import { newRunId } from '@/lib/format';
import { useAgentEvents } from '@/lib/useAgentEvents';
import { useAsyncData } from '@/lib/hooks';
import type { AskResponse, AskSource } from '@/types/ipc';

/**
 * 把 source.kind + id 解析为详情页路由。
 * entity/claim/document 有独立详情页；chunk 等无法可靠下钻时返回 null（仅展示文本）。
 */
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

export function AskPage() {
  const appInfo = useAsyncData(() => app_info(), []);
  const aiEnabled = appInfo.data?.aiEnabled ?? false;

  const [question, setQuestion] = useState('');
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<AskResponse | null>(null);
  const [error, setError] = useState<WikiError | null>(null);
  // 流式：本次运行的 id 与已累积的回答增量（TDD §54）。
  const [runId, setRunId] = useState<string | null>(null);
  const [streamingAnswer, setStreamingAnswer] = useState('');

  useAgentEvents(runId, (event) => {
    if (event.type === 'token_delta') {
      setStreamingAnswer((prev) => prev + event.delta);
    }
  });

  async function handleAsk(event: FormEvent) {
    event.preventDefault();
    const q = question.trim();
    if (!q || loading) return;

    const id = newRunId();
    setRunId(id);
    setStreamingAnswer('');
    setLoading(true);
    setError(null);
    try {
      const res = await ask({ question: q, role: 'auto', runId: id });
      setResult(res);
    } catch (cause: unknown) {
      setError(cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause)));
      setResult(null);
    } finally {
      setStreamingAnswer('');
      setRunId(null);
      setLoading(false);
    }
  }

  const trimmedQuestion = question.trim();

  return (
    <div>
      <PageHeader
        title="Ask"
        subtitle="基于你的知识库提问，回答附带引用与证据（Answer + Citations + Evidence + Context Stats）。"
        actions={
          <Badge tone={aiEnabled ? 'ok' : 'neutral'} title="AI Runtime 是否启用">
            aiEnabled = {String(aiEnabled)}
          </Badge>
        }
      />

      <Card className="p-5">
        <form onSubmit={handleAsk} className="flex flex-col gap-3">
          <label htmlFor="ask-question" className="text-[11px] font-medium text-muted">
            提问
          </label>
          <div className="flex gap-2">
            <Input
              id="ask-question"
              value={question}
              onChange={(e) => setQuestion(e.target.value)}
              placeholder="Ask your wiki…（例如：X 与 Y 的关系是什么？）"
              disabled={loading}
              autoComplete="off"
            />
            <Button type="submit" variant="primary" loading={loading} disabled={!trimmedQuestion}>
              <SparkIcon className="h-3.5 w-3.5" />
              Ask
            </Button>
          </div>
        </form>

        {loading ? (
          streamingAnswer ? (
            <Card className="mt-4 p-5">
              <h3 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-muted">
                Answer（流式生成中…）
              </h3>
              <div className="whitespace-pre-wrap break-words text-sm leading-relaxed text-ink/90">
                {streamingAnswer}
              </div>
            </Card>
          ) : (
            <div className="mt-4 flex items-center gap-2 text-xs text-muted">
              <Spinner className="h-3.5 w-3.5" />
              正在向知识库提问…
            </div>
          )
        ) : null}

        {error ? <ErrorNotice error={error} className="mt-4" /> : null}

        {!loading && !error && result && !result.enabled ? (
          <div className="mt-4 flex items-start gap-3 rounded-lg border border-warn/30 bg-warn/10 p-3">
            <span className="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-warn/15 text-warn">
              <SparkIcon className="h-4 w-4" />
            </span>
            <div className="min-w-0">
              <p className="text-sm font-medium text-ink">当前无法回答</p>
              <p className="mt-1 text-xs leading-relaxed text-muted">
                {result.note ??
                  'AI 未启用或问答链路暂不可用，因此不展示任何回答或引用。请确认已配置 WIKIYA_API_KEY 并启用 AI Runtime。'}
              </p>
            </div>
          </div>
        ) : null}
      </Card>

      {!loading && !error && result && result.enabled ? (
        <div className="mt-4 space-y-4">
          <Card className="p-5">
            <h3 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-muted">Answer</h3>
            <div className="whitespace-pre-wrap break-words text-sm leading-relaxed text-ink/90">
              {result.answer}
            </div>
          </Card>

          {result.sources.length > 0 ? (
            <Card className="p-5">
              <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-muted">
                Sources（{result.sources.length}）
              </h3>
              <ul className="space-y-3">
                {result.sources.map((source) => {
                  const path = sourcePath(source);
                  const titleNode = path ? (
                    <Link to={path} className="text-accent hover:underline">
                      {source.title}
                    </Link>
                  ) : (
                    <span className="text-ink">{source.title}</span>
                  );
                  return (
                    <li key={`${source.kind}-${source.id}-${source.index}`} className="rounded-lg border border-line bg-canvas p-3">
                      <div className="flex items-baseline gap-2 text-sm">
                        <span className="font-mono text-xs text-muted">[{source.index}]</span>
                        {titleNode}
                        <Badge tone="neutral" className="shrink-0">
                          {source.kind}
                        </Badge>
                      </div>
                      <p className="mt-1.5 whitespace-pre-wrap break-words text-[11px] leading-relaxed text-muted">
                        {source.snippet}
                      </p>
                    </li>
                  );
                })}
              </ul>
            </Card>
          ) : null}

          {result.contextStats ? (
            <Card className="p-5">
              <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-muted">
                Context Stats
              </h3>
              <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
                <ContextStat label="Total Tokens" value={result.contextStats.totalTokens} />
                <ContextStat label="Loaded Tokens" value={result.contextStats.loadedTokens} />
                <ContextStat label="Items" value={result.contextStats.itemCount} />
                <ContextStat
                  label="Compression"
                  value={formatCompressionRatio(result.contextStats.compressionRatio)}
                />
              </div>
              {result.contextStats.truncated ? (
                <p className="mt-3 flex items-center gap-1.5 text-[11px] text-warn">
                  <AskIcon className="h-3.5 w-3.5" />
                  部分检索结果因超预算被截断。
                </p>
              ) : null}
            </Card>
          ) : null}
        </div>
      ) : null}

      <div className="mt-4 flex items-center gap-2 text-xs text-muted">
        <AskIcon className="h-4 w-4" />
        其他可用路径：Search（本地词法检索）、Knowledge（实体 / Claim 浏览）、Review（知识变更决策）。
      </div>
    </div>
  );
}

/**
 * 诚实兜底：旧版后端的 ContextStats 可能没有 compressionRatio 字段
 * （IPC 契约演进期的运行时数据），缺省按 1.0 展示，绝不让 UI 崩溃。
 */
function formatCompressionRatio(value: number | undefined): string {
  const ratio = typeof value === 'number' && Number.isFinite(value) ? value : 1;
  return `${ratio.toFixed(2)}x`;
}

function ContextStat({ label, value }: { label: string; value: number | string }) {
  return (
    <div className="rounded-lg border border-line bg-canvas p-3">
      <p className="text-[10px] uppercase tracking-wider text-muted">{label}</p>
      <p className="mt-1 font-mono text-sm text-ink">{value}</p>
    </div>
  );
}
