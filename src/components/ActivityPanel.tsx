import { memo, useCallback, useEffect, useRef, useState } from 'react';
import { Link } from 'react-router-dom';

import { Badge } from '@/components/ui/Badge';
import { Card } from '@/components/ui/Card';
import { Spinner } from '@/components/ui/Spinner';
import { get_run_trace, list_documents, list_extraction_runs } from '@/lib/api';
import { isTerminal, STAGE_LABEL, STATUS_LABEL, STATUS_TONE } from '@/lib/extraction';
import { useAsyncData } from '@/lib/hooks';
import { useRunEvents } from '@/lib/useRunEvents';
import type { DocumentSummary, ExtractionRunDto } from '@/types/ipc';

/**
 * Activity —— 抽取运行历史（EXTRACTION-001）。
 *
 * 列出最近的 Extraction Run，实时反映后台任务的进度 / 状态；
 * 点击可跳到对应文档。任务持久化在 `extraction_runs` 表，
 * 因此页面关了 / 应用重启后再回来，历史依然在。
 */
export function ActivityPanel() {
  const [runs, setRuns] = useState<ExtractionRunDto[]>([]);
  const [titles, setTitles] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(() => {
    list_extraction_runs({ limit: 12 })
      .then(setRuns)
      .catch(() => {})
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    refresh();
    list_documents({ limit: 100 })
      .then((docs: DocumentSummary[]) => {
        const map: Record<string, string> = {};
        for (const doc of docs) map[doc.id] = doc.title;
        setTitles(map);
      })
      .catch(() => {});
  }, [refresh]);

  // 任意统一 Run 事件都应反映到列表，但必须**合并**：Run 事件是逐批 /
  // 逐 token 触发的，逐个整表刷新会持续占用主线程（Ask 流式时的
  // tokenDelta 尤其密集，而它并不改变 Run 概览）。
  // 因此：忽略 tokenDelta + 400ms 节流合并刷新。
  const timerRef = useRef<number | null>(null);
  const scheduleRefresh = useCallback(() => {
    if (timerRef.current !== null) return;
    timerRef.current = window.setTimeout(() => {
      timerRef.current = null;
      refresh();
    }, 400);
  }, [refresh]);

  useEffect(
    () => () => {
      if (timerRef.current !== null) window.clearTimeout(timerRef.current);
    },
    [],
  );

  // 任意统一 Run 事件都触发一次（节流后的）刷新：抽取/Agent/Skill 全部可见。
  //
  // PERF-09：只保留**真的会改变这张列表**的事件。
  // - `tokenDelta`：Ask 的流式文本，逐 token 触发且与列表无关；
  // - `toolCalled` / `toolCompleted`：Research 的中间步骤，列表里不显示。
  // 两者都曾让整表按 400ms 节奏白刷一遍。
  useRunEvents(null, (event) => {
    if (event.kind === 'tokenDelta') return;
    if (event.kind === 'toolCalled' || event.kind === 'toolCompleted') return;
    scheduleRefresh();
  });

  return (
    <section>
      <div className="mb-3 flex items-center justify-between">
        <h2 className="text-[11px] font-semibold uppercase tracking-wider text-muted">运行记录</h2>
        {loading ? <Spinner className="h-3.5 w-3.5 text-muted" /> : null}
      </div>

      {!loading && runs.length === 0 ? (
        <p className="text-[11px] text-muted/80">
          还没有抽取任务。在文档里点「分析知识」，或用「捕获」自动抽取后会出现在这里。
        </p>
      ) : null}

      <div className="space-y-2">
        {runs.map((run) => (
          <ActivityItem key={run.id} run={run} titles={titles} />
        ))}
      </div>
    </section>
  );
}

interface ActivityItemProps {
  run: ExtractionRunDto;
  titles: Record<string, string>;
}

/**
 * PERF-09：props 浅比较**故意不比对象身份**。
 *
 * 列表每 400ms 整表刷新一次，`setRuns` 拿到的每个 `run` 都是反序列化出来的新
 * 对象，引用全不相等——默认的 `React.memo` 会因此判定"全都变了"，12 条全部
 * 重渲染。但列表真正显示的只有 status / stage / processedChunks /
 * totalChunks / candidatesFound / changesFound 这几个字段，其余（resultJson、
 * usage、时间戳等）与渲染无关。
 *
 * 只比这六个字段：抽取期间通常只有正在跑的那一条在变，其余 11 条被跳过。
 */
function sameVisibleRun(a: ExtractionRunDto, b: ExtractionRunDto): boolean {
  return (
    a.id === b.id &&
    a.status === b.status &&
    a.stage === b.stage &&
    a.processedChunks === b.processedChunks &&
    a.totalChunks === b.totalChunks &&
    a.candidatesFound === b.candidatesFound &&
    a.changesFound === b.changesFound
  );
}

/** 单条运行记录：点击展开运行详情（get_run_trace，M13）。 */
const ActivityItem = memo(function ActivityItem({ run, titles }: ActivityItemProps) {
  const [expanded, setExpanded] = useState(false);
  const trace = useAsyncData(
    () => (expanded ? get_run_trace({ id: run.id }) : Promise.resolve(null)),
    [expanded, run.id],
    expanded,
  );
  const title = titles[run.documentId] ?? run.documentId.slice(0, 8);
  const running = !isTerminal(run.status);
  const tone = STATUS_TONE[run.status] ?? 'neutral';

  return (
    <Card className="p-3">
      <button
        type="button"
        className="flex w-full items-start justify-between gap-3 text-left"
        onClick={() => setExpanded((v) => !v)}
      >
        <div className="min-w-0 space-y-1">
          <div className="flex flex-wrap items-center gap-1.5">
            <Badge tone={tone}>{STATUS_LABEL[run.status] ?? run.status}</Badge>
            <span className="truncate text-sm text-ink">{title}</span>
          </div>
          <p className="text-[11px] leading-relaxed text-muted">
            {running && run.stage ? `${STAGE_LABEL[run.stage] ?? run.stage}` : null}
            {run.totalChunks > 0 ? ` · ${run.processedChunks}/${run.totalChunks} 块` : null}
            {!running && run.candidatesFound > 0 ? ` · ${run.candidatesFound} 候选` : null}
            {!running && run.changesFound > 0 ? ` · ${run.changesFound} 变更` : null}
            <span className="text-muted/50"> · {expanded ? '收起详情' : '展开详情'}</span>
          </p>
        </div>
        <Link
          to={`/documents/${run.documentId}`}
          className="shrink-0 text-[11px] text-accent hover:underline"
          onClick={(event) => event.stopPropagation()}
        >
          查看
        </Link>
      </button>
      {expanded && trace.data ? (
        <div className="mt-2 space-y-1 border-t border-line pt-2 text-[11px] text-muted">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-mono text-muted/70">{run.id.slice(0, 8)}</span>
            <span>{trace.data.actor || trace.data.runType}</span>
            <span>开始 {trace.data.startedAt}</span>
            {trace.data.finishedAt ? <span>结束 {trace.data.finishedAt}</span> : null}
          </div>
          {trace.data.usage ? (
            <div className="text-muted/80">
              真实消耗：输入 {trace.data.usage.inputTokens} · 输出{' '}
              {trace.data.usage.outputTokens} tokens
              {trace.data.usage.embeddingTokens > 0
                ? ` · 向量化 ${trace.data.usage.embeddingTokens}`
                : ''}
              {trace.data.usage.retries > 0 ? ` · 重试 ${trace.data.usage.retries} 次` : ''}
              {trace.data.costUsd !== undefined
                ? ` · 约 $${trace.data.costUsd.toFixed(4)}`
                : ''}
            </div>
          ) : null}
          {trace.data.agentSteps.length > 0 ? (
            <ul className="space-y-0.5">
              {trace.data.agentSteps.map((step) => (
                <li key={step.stepIndex}>
                  <span className="font-mono">#{step.stepIndex}</span> {step.name}
                  {step.status === 'failed' ? '（失败）' : ''}
                </li>
              ))}
            </ul>
          ) : null}
        </div>
      ) : null}
    </Card>
  );
},
(prev, next) => prev.titles === next.titles && sameVisibleRun(prev.run, next.run));
