import { useState, type ReactNode } from 'react';
import { Link } from 'react-router-dom';

import { Badge } from '@/components/ui/Badge';
import { Card } from '@/components/ui/Card';
import { ErrorNotice } from '@/components/ErrorNotice';
import { ChevronDownIcon, ChevronRightIcon } from '@/components/icons';
import { Spinner } from '@/components/ui/Spinner';
import { get_claim_trace } from '@/lib/api';
import { formatDateTime } from '@/lib/format';
import { relationshipLabel } from '@/lib/status';
import { useAsyncData } from '@/lib/hooks';

/**
 * Claim 溯源卡片（M8：Knowledge Trace）。
 *
 * 从当前知识**逐层下钻**到源头，而不是一次铺开所有卡片：
 *   Claim → Review Decision（候选）→ 产生过程 Run（skill@version）→ 来源 Evidence/Chunk → 演化。
 * 只呈现**知识关系（Provenance）**，不掺杂 Run 事件日志（Event）。
 */
export function ClaimTraceCard({ claimId }: { claimId: string }) {
  const trace = useAsyncData(() => get_claim_trace({ id: claimId }), [claimId]);

  if (trace.error) {
    return <ErrorNotice error={trace.error} />;
  }
  if (trace.loading && !trace.data) {
    return (
      <div className="flex items-center gap-2 text-xs text-muted">
        <Spinner className="h-3.5 w-3.5" />
        加载溯源…
      </div>
    );
  }
  if (!trace.data) {
    return null;
  }

  const { candidate, run, evidences, evolutions } = trace.data;
  const hasAny = Boolean(candidate) || Boolean(run) || evidences.length > 0 || evolutions.length > 0;

  return (
    <div className="space-y-2">
      <p className="text-[11px] leading-relaxed text-muted">
        从当前知识逐层下钻到源头（知识关系溯源，不含运行日志）。
      </p>

      {!hasAny ? (
        <Card className="p-3 text-[11px] text-muted">
          这条知识由手动录入，暂无自动抽取溯源。
        </Card>
      ) : null}

      {/* 1) Review Decision：候选经人工 Review 接受 → 落库为 Claim */}
      {candidate ? (
        <TraceStep
          title="Review Decision"
          summary={`候选经人工 Review 接受 · ${supportLabel(candidate.supportLevel)}`}
          badge={
            <Badge tone={candidate.status === 'accepted' ? 'ok' : 'neutral'}>
              {candidate.status}
            </Badge>
          }
        >
          <div className="space-y-1 text-[11px] text-muted">
            <div>
              候选 <IdText value={candidate.candidateId} /> 经人工 Review 接受，落库为当前 Claim。
            </div>
            <div>
              候选状态：{candidate.status} · 创建于 {formatDateTime(candidate.createdAt)}
            </div>
            <div>
              原文锚定：{supportLabel(candidate.supportLevel)}
              {candidate.supportLevel === 'directly'
                ? '（quote 逐字落在来源切片，可追溯）'
                : candidate.supportLevel === 'partially'
                  ? '（有切片锚点但无逐字引用）'
                  : candidate.supportLevel === 'unsupported'
                    ? '（无切片锚点，无法验证）'
                    : ''}
            </div>
          </div>
        </TraceStep>
      ) : null}

      {/* 2) 产生过程 Run：skill@version + 抽取明细（不展开 agent_steps 事件日志） */}
      {run ? (
        <TraceStep
          title="产生过程 Run"
          summary={`${run.actor || run.runType} · ${run.runType} · ${run.status}`}
          badge={
            <Badge tone={run.status === 'completed' ? 'ok' : 'warn'}>{run.status}</Badge>
          }
        >
          <div className="space-y-1 text-[11px] text-muted">
            <div>
              Run <IdText value={run.id} /> · 类型 {run.runType} · actor {run.actor}
            </div>
            {run.parentRunId ? (
              <div>父 Run（Agent）：<IdText value={run.parentRunId} /></div>
            ) : null}
            <div>
              开始 {formatDateTime(run.startedAt)}
              {run.finishedAt ? ` · 结束 ${formatDateTime(run.finishedAt)}` : ''}
            </div>
            {run.extractionRun ? (
              <div className="mt-1 rounded-md bg-elevated/60 p-2">
                抽取明细：处理切片 {run.extractionRun.processedChunks}/
                {run.extractionRun.totalChunks} · 发现候选{' '}
                {run.extractionRun.candidatesFound} · 变更{' '}
                {run.extractionRun.changesFound}
              </div>
            ) : null}
          </div>
        </TraceStep>
      ) : null}

      {/* 3) 来源 Source：Evidence → Chunk → Document（默认展开，一键追到源头） */}
      {evidences.length > 0 ? (
        <TraceStep
          title="来源 Source"
          summary={`${evidences.length} 条证据 · ${dedupeTitles(evidences)}`}
          defaultOpen
        >
          <ul className="space-y-2">
            {evidences.map((evidence) => (
              <li key={evidence.evidenceId} className="text-[11px]">
                <div className="flex flex-wrap items-center gap-2">
                  <Link
                    to={`/documents/${evidence.documentId}`}
                    className="text-accent hover:underline"
                  >
                    {evidence.documentTitle || evidence.documentId.slice(0, 8)}
                  </Link>
                  {evidence.chunkIndex !== null && evidence.chunkIndex !== undefined ? (
                    <span className="text-muted/70">切片 #{evidence.chunkIndex}</span>
                  ) : null}
                </div>
                {evidence.quote ? (
                  <blockquote className="mt-1 border-l-2 border-line pl-2 text-muted/80">
                    “{evidence.quote}”
                  </blockquote>
                ) : null}
                {evidence.chunkText ? (
                  <pre className="mt-1 max-h-32 overflow-auto whitespace-pre-wrap rounded-md bg-elevated/60 p-2 text-muted/70">
                    {evidence.chunkText}
                  </pre>
                ) : null}
              </li>
            ))}
          </ul>
        </TraceStep>
      ) : null}

      {/* 4) 演化 Evolution：该 Claim 参与的关系（可下钻到关联 Claim 的溯源） */}
      {evolutions.length > 0 ? (
        <TraceStep title="演化 Evolution" summary={`参与 ${evolutions.length} 条关系`}>
          <ul className="space-y-1 text-[11px] text-muted">
            {evolutions.map((evo) => {
              const otherId =
                evo.sourceClaimId === claimId ? evo.targetClaimId : evo.sourceClaimId;
              return (
                <li key={evo.relationId} className="flex flex-wrap items-center gap-2">
                  <Badge tone={evo.status === 'accepted' ? 'ok' : 'neutral'}>
                    {relationshipLabel(evo.relationship)}
                  </Badge>
                  <span>{evo.status}</span>
                  {evo.reason ? <span className="text-muted/70">· {evo.reason}</span> : null}
                  <Link
                    to={`/claims/${otherId}`}
                    className="ml-auto text-accent hover:underline"
                  >
                    关联 Claim ↗
                  </Link>
                </li>
              );
            })}
          </ul>
        </TraceStep>
      ) : null}
    </div>
  );
}

/** 可展开/收起的溯源步骤。 */
function TraceStep({
  title,
  summary,
  badge,
  defaultOpen = false,
  children,
}: {
  title: string;
  summary: string;
  badge?: ReactNode;
  defaultOpen?: boolean;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div className="overflow-hidden rounded-lg border border-line">
      <button
        type="button"
        onClick={() => setOpen((prev) => !prev)}
        className="flex w-full items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-elevated/40"
      >
        {open ? (
          <ChevronDownIcon className="h-3.5 w-3.5 shrink-0 text-muted" />
        ) : (
          <ChevronRightIcon className="h-3.5 w-3.5 shrink-0 text-muted" />
        )}
        <span className="text-[11px] font-semibold uppercase tracking-wider text-muted">
          {title}
        </span>
        {badge}
        <span className="ml-auto truncate pl-2 text-xs text-muted/80">{summary}</span>
      </button>
      {open ? (
        <div className="border-t border-line px-3 py-2">{children}</div>
      ) : null}
    </div>
  );
}

/** 短 ID 展示。 */
function IdText({ value }: { value: string }) {
  return <span className="font-mono text-[10px] text-muted/80">{value}</span>;
}

/** 来源文档去重标题。 */
function dedupeTitles(
  evidences: { documentTitle: string; documentId: string }[],
): string {
  const titles = evidences.map((e) => e.documentTitle || e.documentId.slice(0, 8));
  return Array.from(new Set(titles)).join('、');
}

/** 候选原文锚定支持度文案（PR-03）。 */
function supportLabel(level?: string): string {
  switch (level) {
    case 'directly':
      return '已锚定原文';
    case 'partially':
      return '部分锚定';
    case 'unsupported':
      return '未锚定原文';
    default:
      return '未知';
  }
}
