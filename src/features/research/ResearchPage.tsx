import { FormEvent, useState } from 'react';

import { ErrorNotice } from '@/components/ErrorNotice';
import { ResearchIcon, SparkIcon } from '@/components/icons';
import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { Input } from '@/components/ui/Input';
import { Spinner } from '@/components/ui/Spinner';
import { PageHeader } from '@/components/PageHeader';
import { app_info, list_research_tasks, start_research, WikiError } from '@/lib/api';
import { newRunId } from '@/lib/format';
import { useAgentEvents } from '@/lib/useAgentEvents';
import { useAsyncData } from '@/lib/hooks';
import type { ResearchReport, ResearchTaskCard } from '@/types/ipc';

/** 实时步骤（来自 agent-events 的 tool_completed）。 */
interface LiveStep {
  tool: string;
  ok: boolean;
  summary: string;
}

function StatusBadge({ status }: { status: string }) {
  const tone =
    status === 'completed' ? 'ok' : status === 'failed' ? 'danger' : status === 'running' ? 'warn' : 'neutral';
  return <Badge tone={tone}>{status}</Badge>;
}

/**
 * Research 多步研究（Phase 6 真实版 + 流式过程，TDD §54/§66）。
 *
 * 诚实边界：研究结果（Findings）只进 Review 队列，绝不直接落库 ——
 * UI 必须明示这一点，避免用户误以为结论已成为知识。
 */
export function ResearchPage() {
  const appInfo = useAsyncData(() => app_info(), []);
  const aiEnabled = appInfo.data?.aiEnabled ?? false;
  const tasks = useAsyncData(() => list_research_tasks(), []);

  const [question, setQuestion] = useState('');
  const [loading, setLoading] = useState(false);
  const [report, setReport] = useState<ResearchReport | null>(null);
  const [error, setError] = useState<WikiError | null>(null);
  // 流式过程：本次运行 id、已完成步骤与正在生成的 Findings 增量。
  const [runId, setRunId] = useState<string | null>(null);
  const [liveSteps, setLiveSteps] = useState<LiveStep[]>([]);
  const [findingsPreview, setFindingsPreview] = useState('');

  useAgentEvents(runId, (event) => {
    switch (event.type) {
      case 'tool_completed':
        setLiveSteps((steps) => [
          ...steps,
          { tool: event.tool, ok: event.ok, summary: event.summary },
        ]);
        break;
      case 'token_delta':
        setFindingsPreview((prev) => prev + event.delta);
        break;
      default:
        break;
    }
  });

  async function handleStart(event: FormEvent) {
    event.preventDefault();
    const q = question.trim();
    if (!q || loading) return;

    const id = newRunId();
    setRunId(id);
    setLiveSteps([]);
    setFindingsPreview('');
    setLoading(true);
    setError(null);
    try {
      const result = await start_research({ question: q, runId: id });
      setReport(result);
    } catch (cause: unknown) {
      setError(cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause)));
      setReport(null);
    } finally {
      setFindingsPreview('');
      setRunId(null);
      setLoading(false);
    }
  }

  const trimmedQuestion = question.trim();

  return (
    <div>
      <PageHeader
        title="Research"
        subtitle="多步知识构建：提出研究问题，Agent 检索、下钻、比较并汇总为候选知识。"
        actions={
          <Badge tone={aiEnabled ? 'ok' : 'neutral'} title="AI Runtime 是否启用">
            aiEnabled = {String(aiEnabled)}
          </Badge>
        }
      />

      <Card className="p-5">
        <form onSubmit={handleStart} className="flex flex-col gap-3">
          <label htmlFor="research-question" className="text-[11px] font-medium text-muted">
            研究问题
          </label>
          <div className="flex gap-2">
            <Input
              id="research-question"
              value={question}
              onChange={(e) => setQuestion(e.target.value)}
              placeholder="例如：Rust 的 async trait 支持演进到哪一步了？"
              disabled={loading}
              autoComplete="off"
            />
            <Button type="submit" variant="primary" loading={loading} disabled={!trimmedQuestion}>
              <SparkIcon className="h-3.5 w-3.5" />
              Start Research
            </Button>
          </div>
        </form>

        {loading ? (
          <div className="mt-4 space-y-3">
            <div className="flex items-center gap-2 text-xs text-muted">
              <Spinner className="h-3.5 w-3.5" />
              正在检索知识库、收集证据并汇总…
            </div>

            {liveSteps.length > 0 ? (
              <ol className="space-y-2">
                {liveSteps.map((step, index) => (
                  <li
                    key={`${step.tool}-${index}`}
                    className="flex items-start gap-2 rounded-lg border border-line bg-canvas p-3"
                  >
                    <Badge tone={step.ok ? 'ok' : 'warn'} className="shrink-0">
                      {step.tool}
                    </Badge>
                    <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-muted">
                      {step.summary}
                    </span>
                  </li>
                ))}
              </ol>
            ) : null}

            {findingsPreview ? (
              <div className="rounded-lg border border-line bg-canvas p-4">
                <p className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-muted">
                  Findings（流式生成中…）
                </p>
                <div className="whitespace-pre-wrap break-words text-sm leading-relaxed text-ink/90">
                  {findingsPreview}
                </div>
              </div>
            ) : null}
          </div>
        ) : null}

        {error ? <ErrorNotice error={error} className="mt-4" /> : null}

        {!loading && !error && report && !report.enabled ? (
          <div className="mt-4 flex items-start gap-3 rounded-lg border border-warn/30 bg-warn/10 p-3">
            <span className="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-warn/15 text-warn">
              <SparkIcon className="h-4 w-4" />
            </span>
            <div className="min-w-0">
              <p className="text-sm font-medium text-ink">当前无法研究</p>
              <p className="mt-1 text-xs leading-relaxed text-muted">
                {report.note ??
                  'AI 未启用或研究链路暂不可用，因此不会执行任何研究任务。请确认已在 Settings 配置 AI 运行时。'}
              </p>
            </div>
          </div>
        ) : null}
      </Card>

      {!loading && !error && report && report.enabled ? (
        <div className="mt-4 space-y-4">
          <Card className="p-5">
            <h3 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-muted">
              Findings
            </h3>
            <div className="whitespace-pre-wrap break-words text-sm leading-relaxed text-ink/90">
              {report.answer}
            </div>
          </Card>

          {report.steps.length > 0 ? (
            <Card className="p-5">
              <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-muted">
                Process（{report.steps.length} 步）
              </h3>
              <ol className="space-y-2">
                {report.steps.map((step, index) => (
                  <li
                    key={`${step.tool}-${index}`}
                    className="rounded-lg border border-line bg-canvas p-3"
                  >
                    <div className="flex items-baseline gap-2 text-sm">
                      <span className="font-mono text-xs text-muted">{index + 1}.</span>
                      <Badge tone="accent" className="shrink-0">
                        {step.tool}
                      </Badge>
                    </div>
                    <pre className="mt-1.5 overflow-x-auto whitespace-pre-wrap break-words font-mono text-[11px] leading-relaxed text-muted">
                      {step.summary}
                    </pre>
                  </li>
                ))}
              </ol>
            </Card>
          ) : null}

          {report.reviewId ? (
            <div className="flex items-start gap-3 rounded-lg border border-ok/30 bg-ok/10 p-3 text-xs leading-relaxed text-muted">
              <ResearchIcon className="mt-0.5 h-4 w-4 shrink-0 text-ok" />
              <span>
                研究结论已作为待审 Findings 进入 <strong className="text-ink">Review</strong>{' '}
                队列（记录 {report.reviewId}）。在你确认之前，它不会成为知识库的一部分。
              </span>
            </div>
          ) : null}
        </div>
      ) : null}

      <section>
        <h2 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-muted">
          历史任务
        </h2>
        {tasks.loading && !tasks.data ? (
          <div className="flex items-center gap-2 text-xs text-muted">
            <Spinner className="h-3.5 w-3.5" />
            加载历史…
          </div>
        ) : null}
        {tasks.data && tasks.data.length === 0 ? (
          <p className="text-xs text-muted">还没有研究任务。</p>
        ) : null}
        {tasks.data && tasks.data.length > 0 ? (
          <Card className="divide-y divide-line">
            {tasks.data.map((task: ResearchTaskCard) => (
              <div key={task.id} className="flex items-start gap-3 px-4 py-3">
                <span className="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-surface text-muted">
                  <ResearchIcon className="h-3.5 w-3.5" />
                </span>
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm text-ink">{task.question}</p>
                  {task.summary ? (
                    <p className="mt-0.5 line-clamp-2 text-[11px] leading-relaxed text-muted">
                      {task.summary}
                    </p>
                  ) : null}
                </div>
                <div className="flex shrink-0 flex-col items-end gap-1">
                  <StatusBadge status={task.status} />
                  <span className="text-[10px] text-muted/70">
                    {task.createdAt.replace('T', ' ').slice(0, 19)}
                  </span>
                </div>
              </div>
            ))}
          </Card>
        ) : null}
      </section>

      <div className="mt-4 flex items-center gap-2 text-xs text-muted">
        <ResearchIcon className="h-4 w-4" />
        研究结果只进 Review，不直接落库；当前可用的替代路径：Inbox 捕获、Knowledge 手动录入、Review 决策。
      </div>
    </div>
  );
}
