import { useCallback, useEffect, useRef, useState } from 'react';

import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { ErrorNotice } from '@/components/ErrorNotice';
import { Spinner } from '@/components/ui/Spinner';
import { SparkIcon } from '@/components/icons';
import {
  analyze_document,
  cancel_extraction,
  create_claim,
  get_extraction_run,
  list_extraction_runs,
  start_extraction,
  WikiError,
} from '@/lib/api';
import { useExtractionEvents } from '@/lib/useExtractionEvents';
import { isTerminal, STATUS_LABEL, STAGE_LABEL } from '@/lib/extraction';
import type {
  CreateClaimInput,
  ExtractedClaim,
  ExtractionEvent,
  ExtractionReport,
  ExtractionRunDto,
} from '@/types/ipc';

interface ExtractionPanelProps {
  documentId: string;
  /** 抽取并落库后回调（用于刷新文档详情）。 */
  onClaimsAccepted: () => void;
}

function itemKey(item: ExtractedClaim): string {
  return `${item.subject}|${item.predicate}|${item.sentence ?? ''}`;
}

/**
 * AI 抽取面板（EXTRACTION-001：异步 Run 化）。
 *
 * 点击「分析知识」后**立即**拿到 `run_id` 返回，真正的抽取在后台跑；
 * 面板订阅 `extraction-events` 实时呈现阶段与进度，并持久化到数据库——
 * 页面关了 / 应用关了再回来都能续上（重开同文档会自动 resume 未结束的 Run）。
 *
 * 候选仍只预览、不擅自落库：用户逐条或批量「接受」后才走 `create_claim`
 * + `analyze_document`（复用 Review / Evolution 流程）。
 */
export function ExtractionPanel({ documentId, onClaimsAccepted }: ExtractionPanelProps) {
  const [run, setRun] = useState<ExtractionRunDto | null>(null);
  const [runId, setRunId] = useState<string | null>(null);
  const [report, setReport] = useState<ExtractionReport | null>(null);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<WikiError | null>(null);
  const [accepted, setAccepted] = useState<string[]>([]);
  const [accepting, setAccepting] = useState(false);
  const pendingRef = useRef<Set<string>>(new Set());
  const [pendingKeys, setPendingKeys] = useState<string[]>([]);
  const acceptingRef = useRef(false);

  const applyRun = useCallback((next: ExtractionRunDto) => {
    setRun(next);
    if (isTerminal(next.status) && next.resultJson) {
      try {
        setReport(JSON.parse(next.resultJson) as ExtractionReport);
      } catch {
        // 结果 JSON 损坏：忽略，保留进度状态，不让面板崩。
      }
    }
  }, []);

  const onEvent = useCallback(
    (event: ExtractionEvent) => {
      setRun((prev) => {
        if (!prev) return prev;
        switch (event.type) {
          case 'started':
            return { ...prev, status: 'running' };
          case 'stage_changed':
            return { ...prev, stage: event.stage ?? prev.stage };
          case 'progress':
            return {
              ...prev,
              processedChunks: event.processed ?? prev.processedChunks,
              totalChunks: event.total ?? prev.totalChunks,
            };
          case 'candidate_found':
            return { ...prev, candidatesFound: event.count ?? prev.candidatesFound };
          case 'comparison_completed':
            return { ...prev, changesFound: event.changes ?? prev.changesFound };
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
      // 终态事件：拉一次完整快照（带 result_json）并解析结果。
      if (
        event.type === 'completed' ||
        event.type === 'failed' ||
        event.type === 'cancelled'
      ) {
        get_extraction_run({ id: event.runId }).then(applyRun).catch(() => {});
      }
    },
    [applyRun],
  );

  useExtractionEvents(runId, onEvent);

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
        }
      })
      .catch(() => {});
  }, [documentId]);

  // run_id 确定后，拉一次初始快照（覆盖 resume 之外的创建瞬间）。
  useEffect(() => {
    if (!runId) return;
    get_extraction_run({ id: runId }).then(applyRun).catch(() => {});
  }, [runId, applyRun]);

  // 兜底轮询：抽取进行中时每 1.2s 拉一次快照，避免事件遗漏导致 UI 卡住。
  useEffect(() => {
    if (!runId || (run && isTerminal(run.status))) return;
    const timer = setInterval(() => {
      get_extraction_run({ id: runId }).then(applyRun).catch(() => {});
    }, 1200);
    return () => clearInterval(timer);
  }, [runId, run, applyRun]);

  async function startRun() {
    setStarting(true);
    setError(null);
    setReport(null);
    setAccepted([]);
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

  async function acceptItem(item: ExtractedClaim) {
    const key = itemKey(item);
    if (pendingRef.current.has(key) || accepted.includes(key)) {
      return;
    }
    pendingRef.current.add(key);
    setPendingKeys([...pendingRef.current]);

    const input: CreateClaimInput = {
      subject: item.subject,
      predicate: item.predicate,
      object: item.objectText ?? null,
      content: item.content ?? null,
      claimType: item.claimType ?? 'factual',
      polarity: item.polarity ?? 'positive',
      modality: item.modality ?? 'asserted',
      confidence: item.confidence ?? null,
      documentId,
      chunkId: null,
      quote: item.sourceQuote ?? item.sentence ?? null,
      status: 'candidate',
    };
    try {
      await create_claim(input);
      setAccepted((prev) => (prev.includes(key) ? prev : [...prev, key]));
    } finally {
      pendingRef.current.delete(key);
      setPendingKeys([...pendingRef.current]);
    }
  }

  async function acceptAll() {
    if (!report || acceptingRef.current) {
      return;
    }
    acceptingRef.current = true;
    setAccepting(true);
    setError(null);
    try {
      for (const item of report.extracted) {
        if (item.accepted && !accepted.includes(itemKey(item))) {
          await acceptItem(item);
        }
      }
      await analyze_document({ documentId });
      onClaimsAccepted();
    } catch (cause: unknown) {
      setError(cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause)));
    } finally {
      acceptingRef.current = false;
      setAccepting(false);
    }
  }

  const acceptedSet = new Set(accepted);
  const pendingSet = new Set(pendingKeys);

  const running = run ? !isTerminal(run.status) : false;

  return (
    <section>
      <div className="mb-3 flex items-center justify-between">
        <h3 className="text-[11px] font-semibold uppercase tracking-wider text-muted">AI 抽取</h3>
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
          <div className="flex items-center gap-2 text-xs text-muted">
            <Spinner className="h-3.5 w-3.5" />
            <span>
              {STATUS_LABEL[run.status] ?? run.status}
              {run.stage ? ` · ${STAGE_LABEL[run.stage] ?? run.stage}` : ''}
            </span>
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
        </Card>
      ) : null}

      {run && run.status === 'failed' ? (
        <Card className="border-dashed border-line bg-surface/50 p-4 text-xs text-warn">
          {run.errorMessage ?? '抽取失败。'}
        </Card>
      ) : null}

      {run && run.status === 'interrupted' ? (
        <Card className="border-dashed border-line bg-surface/50 p-4 text-xs text-muted">
          该抽取在上次应用关闭时仍未完成，已被标记为中断。可重新点击「分析知识」再跑一次。
        </Card>
      ) : null}

      {report && !report.enabled ? (
        <Card className="border-dashed border-line bg-surface/50 p-4 text-xs text-muted">
          {report.note ?? 'AI 未启用。'}
        </Card>
      ) : null}

      {report && report.enabled ? (
        report.extracted.length === 0 ? (
          <Card className="border-dashed border-line bg-surface/50 p-4 text-xs text-muted">
            没有可抽取的 Claim（或模型判定本文无可结构化断言的内容）。
          </Card>
        ) : (
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <span className="text-[10px] text-muted">
                共 {report.extracted.filter((item) => item.accepted).length} 条通过校验，
                {report.extracted.filter((item) => !item.accepted).length} 条已被排除
                {run && run.changesFound > 0 ? ` · 约 ${run.changesFound} 条为新增` : ''}
              </span>
              <Button
                size="sm"
                variant="ghost"
                loading={accepting}
                onClick={acceptAll}
                disabled={
                  accepting ||
                  report.extracted.every((item) => acceptedSet.has(itemKey(item)))
                }
              >
                全部接受
              </Button>
            </div>

            {report.extracted.map((item) => {
              const key = itemKey(item);
              const isAccepted = acceptedSet.has(key);
              const isPending = pendingSet.has(key);
              return (
                <Card key={key} className="p-3">
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0 space-y-1">
                      <div className="flex flex-wrap items-center gap-1.5 text-[11px]">
                        <span className="font-medium text-ink">{item.subject}</span>
                        <Badge tone="accent">{item.predicate}</Badge>
                        {item.objectText ? (
                          <span className="text-muted">→ {item.objectText}</span>
                        ) : null}
                      </div>
                      {item.content ? (
                        <p className="text-[11px] leading-relaxed text-muted">{item.content}</p>
                      ) : null}
                      {item.sentence ? (
                        <p className="text-[10px] leading-relaxed text-muted/70">“{item.sentence}”</p>
                      ) : null}
                      {!item.accepted && item.rejectReason ? (
                        <p className="text-[10px] text-warn">{item.rejectReason}</p>
                      ) : null}
                    </div>
                    <Button
                      size="sm"
                      variant={isAccepted ? 'ghost' : 'secondary'}
                      loading={isPending}
                      disabled={!item.accepted || isAccepted || isPending || accepting}
                      onClick={() => acceptItem(item)}
                    >
                      {isAccepted ? '已接受' : isPending ? '接受中' : '接受'}
                    </Button>
                  </div>
                </Card>
              );
            })}
          </div>
        )
      ) : null}
    </section>
  );
}
