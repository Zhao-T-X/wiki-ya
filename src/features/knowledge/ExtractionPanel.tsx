import { useCallback, useEffect, useState } from 'react';

import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { ErrorNotice } from '@/components/ErrorNotice';
import { SparkIcon } from '@/components/icons';
import {
  cancel_extraction,
  decide_candidate,
  get_extraction_run,
  list_candidates,
  list_extraction_runs,
  start_extraction,
  WikiError,
} from '@/lib/api';
import { useRunEvents } from '@/lib/useRunEvents';
import { isTerminal, STAGE_LABEL } from '@/lib/extraction';
import type { CandidateDto, RunEvent, ExtractionRunDto } from '@/types/ipc';

interface ExtractionPanelProps {
  documentId: string;
  /** 抽取并落库后回调（用于刷新文档详情）。 */
  onClaimsAccepted: () => void;
}

/** 阶段顺序（与后端 ExtractionStage 枚举一致）。 */
const STAGE_ORDER = [
  'preparing',
  'chunking',
  'extracting',
  'validating',
  'comparing',
  'finalizing',
] as const;

/**
 * AI 抽取面板（M7：真正可观察的过程 + 候选持久化）。
 *
 * - 阶段时间线：Preparing ✓ → Chunking ✓ → Extracting ● 8/15 → … → Finalizing —
 * - 候选一产生就持久化（M6）：终态后从 `candidates` 表读取，
 *   逐条「接受」（→ Claim + 演化分析）或「拒绝」（留痕）；
 * - 页面关了 / 应用关了再回来都能续上（自动 resume 未结束的 Run）。
 */
export function ExtractionPanel({ documentId, onClaimsAccepted }: ExtractionPanelProps) {
  const [run, setRun] = useState<ExtractionRunDto | null>(null);
  const [runId, setRunId] = useState<string | null>(null);
  const [candidates, setCandidates] = useState<CandidateDto[]>([]);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<WikiError | null>(null);
  const [deciding, setDeciding] = useState<string[]>([]);
  // PERF-04：候选游标分页——`null` 表示已到末页。
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  // PR-03：默认只把"已直接锚定原文(directly)"的候选放进正常 Review 流；
  // partially/unsupported 需人工补证据后才可接受（可一键展开查看）。
  const [groundedOnly, setGroundedOnly] = useState(true);

  const refreshCandidates = useCallback((id: string) => {
    // PERF-04：游标分页——首屏只取一页，避免一次渲染全部候选。
    list_candidates({ id })
      .then((page) => {
        setCandidates(page.items);
        setNextCursor(page.nextCursor ?? null);
      })
      .catch(() => {});
  }, []);

  /** 追加下一页（PERF-04）：追加而非替换，用户点「加载更多」才拉。 */
  const loadMoreCandidates = useCallback(() => {
    if (!runId || !nextCursor) return;
    list_candidates({ id: runId, cursor: nextCursor })
      .then((page) => {
        setCandidates((prev) => [...prev, ...page.items]);
        setNextCursor(page.nextCursor ?? null);
      })
      .catch(() => {});
  }, [runId, nextCursor]);

  const applyRun = useCallback(
    (next: ExtractionRunDto) => {
      setRun(next);
      // 终态：候选已持久化，拉一次完整列表（替代旧的 result_json 解析）。
      if (isTerminal(next.status)) {
        refreshCandidates(next.id);
      }
    },
    [refreshCandidates],
  );

  const onEvent = useCallback(
    (event: RunEvent) => {
      setRun((prev) => {
        if (!prev) return prev;
        switch (event.kind) {
          case 'started':
            return { ...prev, status: 'running' };
          case 'stageChanged':
            return { ...prev, stage: event.stage ?? prev.stage };
          case 'progress':
            return {
              ...prev,
              processedChunks: event.processed ?? prev.processedChunks,
              totalChunks: event.total ?? prev.totalChunks,
            };
          case 'candidateCreated':
            // 逐批增量事件：累加（M6 修正后每批发一次）。
            return {
              ...prev,
              candidatesFound: (prev.candidatesFound ?? 0) + (event.count ?? 0),
            };
          case 'completed':
            return { ...prev, status: 'completed' };
          case 'failed':
            return { ...prev, status: 'failed', errorMessage: event.error ?? prev.errorMessage };
          case 'cancelled':
            return { ...prev, status: 'cancelled' };
          default:
            return prev;
        }
      });
      // 终态事件：拉完整快照 + 候选列表。
      if (
        event.kind === 'completed' ||
        event.kind === 'failed' ||
        event.kind === 'cancelled'
      ) {
        get_extraction_run({ id: event.runId }).then(applyRun).catch(() => {});
      }
    },
    [applyRun],
  );

  useRunEvents(runId, onEvent);

  // 初次进入 / 切换文档：自动 resume 本文档仍在跑的 Run（页面关了再回来也能续上）。
  useEffect(() => {
    if (!documentId) return;
    list_extraction_runs({ limit: 50 })
      .then((runs) => {
        const pending = runs.find(
          (r) => r.documentId === documentId && !isTerminal(r.status),
        );
        if (pending) {
          setRunId(pending.id);
          setRun(pending);
        } else {
          // 没有在跑的 Run：展示本文档最近一次终态 Run 的候选（若有）。
          const last = runs.find(
            (r) => r.documentId === documentId && isTerminal(r.status),
          );
          if (last) {
            setRunId(last.id);
            setRun(last);
          }
        }
      })
      .catch(() => {});
  }, [documentId]);

  // run_id 确定后，拉一次初始快照（覆盖 resume 之外的创建瞬间）。
  useEffect(() => {
    if (!runId) return;
    get_extraction_run({ id: runId }).then(applyRun).catch(() => {});
  }, [runId, applyRun]);

  // 抽取是否进行中（派生布尔量：稳定，可安全用作 effect 依赖）。
  const running = run ? !isTerminal(run.status) : false;

  // 兜底轮询：抽取进行中时每 1.5s 拉一次快照，避免事件遗漏导致 UI 卡住。
  // 依赖稳定的 `running` 布尔量而非 `run` 对象：否则每次进度事件都会重建
  // 定时器，轮询永远等不到间隔触发。
  useEffect(() => {
    if (!runId || !running) return;
    const timer = setInterval(() => {
      get_extraction_run({ id: runId }).then(applyRun).catch(() => {});
    }, 1500);
    return () => clearInterval(timer);
  }, [runId, running, applyRun]);

  async function startRun() {
    setStarting(true);
    setError(null);
    setCandidates([]);
    setNextCursor(null);
    try {
      const id = await start_extraction({ id: documentId });
      setRunId(id);
    } catch (cause: unknown) {
      setError(cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause)));
    } finally {
      setStarting(false);
    }
  }

  async function cancelRun() {
    if (!runId) return;
    try {
      await cancel_extraction({ id: runId });
    } catch (cause: unknown) {
      setError(cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause)));
    }
  }

  async function decide(candidate: CandidateDto, accept: boolean) {
    if (deciding.includes(candidate.id) || candidate.status !== 'pending') {
      return;
    }
    setDeciding((prev) => [...prev, candidate.id]);
    try {
      const updated = await decide_candidate({
        candidateId: candidate.id,
        accept,
      });
      setCandidates((prev) =>
        prev.map((c) => (c.id === updated.id ? updated : c)),
      );
      if (accept) {
        onClaimsAccepted();
      }
    } catch (cause: unknown) {
      setError(cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause)));
    } finally {
      setDeciding((prev) => prev.filter((id) => id !== candidate.id));
    }
  }

  async function acceptAllPending() {
    // PR-03：批量接受只针对当前可见的 pending（默认仅 directly）。
    const pendingList = shown.filter((c) => c.status === 'pending');
    for (const candidate of pendingList) {
      // 顺序执行：后端 accept 自带演化分析，且逐条更新 UI 状态。
      await decide(candidate, true);
    }
  }

  const pendingCount = candidates.filter((c) => c.status === 'pending').length;
  const acceptedCount = candidates.filter((c) => c.status === 'accepted').length;
  const rejectedCount = candidates.filter((c) => c.status === 'rejected').length;
  const decidingSet = new Set(deciding);
  // PR-03：默认仅展示已直接锚定原文的候选（正常 Review 流）。
  const shown = groundedOnly
    ? candidates.filter((c) => c.supportLevel === 'directly')
    : candidates;
  const hiddenCount = candidates.length - shown.length;

  // 阶段时间线：run.stage 之前的 ✓、当前 ●、之后的 —。
  const stageIndex = run ? STAGE_ORDER.indexOf(run.stage as (typeof STAGE_ORDER)[number]) : -1;

  return (
    <section>
      <div className="mb-3 flex items-center justify-between">
        <h3 className="text-meta font-semibold uppercase tracking-wider text-muted">AI 抽取</h3>
        {!running ? (
          <Button size="sm" variant="primary" loading={starting} onClick={startRun}>
            <SparkIcon className="h-3.5 w-3.5" />
            分析知识
          </Button>
        ) : (
          <Button size="sm" variant="ghost" onClick={cancelRun}>
            取消
          </Button>
        )}
      </div>

      {error ? <ErrorNotice error={error} className="mb-3" /> : null}

      {run && running ? (
        <Card className="border-dashed border-line bg-surface/50 p-4">
          {/* 阶段时间线（M7）：每个阶段 ✓ / ● / — 一眼可见 */}
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
            {STAGE_ORDER.map((stage, index) => {
              const done = index < stageIndex || isTerminal(run.status);
              const active = index === stageIndex && !isTerminal(run.status);
              return (
                <span
                  key={stage}
                  className={`flex items-center gap-1 text-[10px] ${
                    active ? 'font-medium text-ink' : done ? 'text-muted' : 'text-muted/50'
                  }`}
                >
                  {done ? '✓' : active ? '●' : '—'}
                  {STAGE_LABEL[stage] ?? stage}
                </span>
              );
            })}
          </div>
          {run.totalChunks > 0 ? (
            <div className="mt-3">
              <div className="mb-1 flex items-center justify-between text-[10px] text-muted">
                <span>进度</span>
                <span>
                  {run.processedChunks} / {run.totalChunks} 块
                </span>
              </div>
              <div className="h-1.5 w-full overflow-hidden rounded-full bg-line">
                <div
                  className="h-full rounded-full bg-accent transition-all"
                  style={{
                    width: `${run.totalChunks > 0 ? (run.processedChunks / run.totalChunks) * 100 : 0}%`,
                  }}
                />
              </div>
            </div>
          ) : null}
          {run.candidatesFound > 0 ? (
            <p className="mt-2 text-[10px] text-muted">已发现 {run.candidatesFound} 条候选</p>
          ) : null}
          {run.usage ? (
            <p className="mt-1 text-[10px] text-muted/80">
              真实消耗：输入 {run.usage.inputTokens} · 输出 {run.usage.outputTokens} tokens
              {run.usage.retries > 0 ? ` · 重试 ${run.usage.retries} 次` : ''}
              {run.costUsd !== undefined ? ` · 约 $${run.costUsd.toFixed(4)}` : ''}
            </p>
          ) : null}
        </Card>
      ) : null}

      {run && run.status === 'failed' ? (
        <Card className="border-dashed border-line bg-surface/50 p-4 text-xs text-warn">
          {run.errorMessage ?? '抽取失败。'}
          {run.processedChunks > 0 && run.totalChunks > 0 ? (
            <span className="text-muted">
              {' '}
              （失败于 {run.processedChunks}/{run.totalChunks} 块；当前知识未受影响）
            </span>
          ) : null}
        </Card>
      ) : null}

      {run && run.status === 'interrupted' ? (
        <Card className="border-dashed border-line bg-surface/50 p-4 text-xs text-muted">
          该抽取在上次应用关闭时仍未完成，已被标记为中断。可重新点击「分析知识」再跑一次。
        </Card>
      ) : null}

      {run && run.status === 'cancelled' ? (
        <Card className="border-dashed border-line bg-surface/50 p-4 text-xs text-muted">
          已取消（{run.processedChunks}/{run.totalChunks} 块）。
        </Card>
      ) : null}

      {/* 候选列表（M6/M7：来自 candidates 表，状态持久化） */}
      {candidates.length > 0 ? (
        <div className="mt-3 space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-[10px] text-muted">
              {shown.length} 条候选
              {acceptedCount > 0 ? ` · 已接受 ${acceptedCount}` : ''}
              {rejectedCount > 0 ? ` · 已拒绝 ${rejectedCount}` : ''}
              {pendingCount > 0 ? ` · 待确认 ${pendingCount}` : ''}
              {run && run.changesFound > 0 ? ` · 预估新增 ${run.changesFound} 条（以确认时演化分析为准）` : ''}
            </span>
            <div className="flex items-center gap-2">
              <button
                type="button"
                onClick={() => setGroundedOnly((v) => !v)}
                className="text-[10px] text-accent hover:underline"
                title="仅显示已直接锚定原文的候选（partially/unsupported 需人工补证据）"
              >
                {groundedOnly ? '显示全部' : '仅已锚定原文'}
              </button>
              {pendingCount > 1 ? (
                <Button
                  size="sm"
                  variant="ghost"
                  loading={deciding.length > 0}
                  onClick={acceptAllPending}
                >
                  全部接受
                </Button>
              ) : null}
            </div>
          </div>

          {hiddenCount > 0 && groundedOnly ? (
            <p className="text-[10px] text-muted/70">
              另有 {hiddenCount} 条未直接锚定原文（partially / unsupported），已折叠——展开后可人工补证据再接受。
            </p>
          ) : null}

          {shown.map((candidate) => {
            const isDeciding = decidingSet.has(candidate.id);
            const badge = supportBadge(candidate.supportLevel);
            return (
              <Card key={candidate.id} className="p-3">
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0 space-y-1">
                    <div className="flex flex-wrap items-center gap-1.5 text-meta">
                      <span className="font-medium text-ink">{candidate.subject}</span>
                      <Badge tone="accent">{candidate.predicate}</Badge>
                      {candidate.objectText ? (
                        <span className="text-muted">→ {candidate.objectText}</span>
                      ) : null}
                      <Badge tone={badge.tone}>{badge.label}</Badge>
                      {candidate.status === 'accepted' ? (
                        <Badge tone="ok">已接受</Badge>
                      ) : candidate.status === 'rejected' ? (
                        <Badge tone="warn">已拒绝</Badge>
                      ) : null}
                    </div>
                    {candidate.content ? (
                      <p className="text-meta leading-relaxed text-muted">{candidate.content}</p>
                    ) : null}
                    {candidate.sourceQuote ? (
                      <p className="text-[10px] leading-relaxed text-muted/70">
                        锚定原文：“{candidate.sourceQuote}”
                      </p>
                    ) : null}
                    {candidate.sentence ? (
                      <p className="text-[10px] leading-relaxed text-muted/70">
                        “{candidate.sentence}”
                      </p>
                    ) : null}
                    {candidate.supportLevel && candidate.supportLevel !== 'directly' ? (
                      <p className="text-[10px] text-warn">
                        ⚠ 未直接锚定原文（{badge.label}）：接受前请确认证据可验证。
                      </p>
                    ) : null}
                    {candidate.rejectReason ? (
                      <p className="text-[10px] text-warn">{candidate.rejectReason}</p>
                    ) : null}
                  </div>
                  {candidate.status === 'pending' ? (
                    <div className="flex shrink-0 gap-1.5">
                      <Button
                        size="sm"
                        variant="secondary"
                        loading={isDeciding}
                        disabled={isDeciding}
                        onClick={() => decide(candidate, true)}
                      >
                        接受
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        disabled={isDeciding}
                        onClick={() => decide(candidate, false)}
                      >
                        拒绝
                      </Button>
                    </div>
                  ) : null}
                </div>
              </Card>
            );
          })}

          {/* PERF-04：还有下一页时才出现「加载更多」——首屏恒为 50 条。 */}
          {nextCursor ? (
            <div className="flex justify-center pt-1">
              <Button size="sm" variant="ghost" onClick={loadMoreCandidates}>
                加载更多候选
              </Button>
            </div>
          ) : null}
        </div>
      ) : null}

      {run && isTerminal(run.status) && run.status !== 'failed' && candidates.length === 0 ? (
        <Card className="mt-3 border-dashed border-line bg-surface/50 p-4 text-xs text-muted">
          没有可抽取的 Claim（或模型判定本文无可结构化断言的内容）。
        </Card>
      ) : null}
    </section>
  );
}

/** 候选原文锚定支持度的展示（PR-03）。 */
function supportBadge(
  level?: 'directly' | 'partially' | 'unsupported',
): { label: string; tone: 'ok' | 'neutral' | 'warn' } {
  switch (level) {
    case 'directly':
      return { label: '已锚定原文', tone: 'ok' };
    case 'partially':
      return { label: '部分锚定', tone: 'neutral' };
    case 'unsupported':
      return { label: '未锚定原文', tone: 'warn' };
    default:
      return { label: '支持度未知', tone: 'neutral' };
  }
}
