import { useState } from 'react';
import { Link } from 'react-router-dom';

import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { ErrorNotice } from '@/components/ErrorNotice';
import { Spinner } from '@/components/ui/Spinner';
import { get_run_trace, run_skill, WikiError } from '@/lib/api';
import { useRunEvents } from '@/lib/useRunEvents';
import { relationshipLabel } from '@/lib/status';
import type { RunEvent } from '@/types/ipc';

interface CorrectionPanelProps {
  documentId: string;
}

interface RelationSummary {
  id: string;
  relationship: string;
  status: string;
  reason?: string;
  suggestedAction?: string;
}

/**
 * 纠正检查面板（M10：Correction Skill 端到端）。
 *
 * 流程完全符合行动计划纪律三——AI 永不直接修改 Claim：
 *   Correction Skill（对比已有知识）
 *     → 提案（claim_relations, candidate 状态）
 *     → Review 队列
 *     → 人类决策（accept → supersedes；旧知识留痕可回滚）
 */
export function CorrectionPanel({ documentId }: CorrectionPanelProps) {
  const [runId, setRunId] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [summary, setSummary] = useState<{ proposals: number; relations: RelationSummary[] } | null>(
    null,
  );
  const [error, setError] = useState<WikiError | null>(null);

  useRunEvents(runId, (event: RunEvent) => {
    if (event.runId !== runId) return;
    if (event.kind === 'completed' || event.kind === 'failed') {
      // 完成：拉 Run Trace 取提案明细（metadata.relations）。
      get_run_trace({ id: runId })
        .then((trace) => {
          const metadata = trace.metadata as {
            skills?: Record<string, { proposals?: number; relations?: RelationSummary[] }>;
          };
          const relations =
            Object.values(metadata.skills ?? {}).find((s) => s.relations)?.relations ?? [];
          setSummary({
            proposals: relations.length,
            relations,
          });
          setRunning(false);
        })
        .catch(() => setRunning(false));
    }
  });

  async function start() {
    setRunning(true);
    setError(null);
    setSummary(null);
    try {
      const id = await run_skill({
        name: 'knowledge-correction',
        input: { documentId },
      });
      setRunId(id);
    } catch (cause: unknown) {
      setError(cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause)));
      setRunning(false);
    }
  }

  return (
    <section>
      <div className="mb-3 flex items-center justify-between">
        <h3 className="text-[11px] font-semibold uppercase tracking-wider text-muted">
          纠正检查
        </h3>
        <Button size="sm" variant="secondary" loading={running} onClick={start}>
          对比已有知识
        </Button>
      </div>

      {error ? <ErrorNotice error={error} className="mb-3" /> : null}

      {running ? (
        <Card className="border-dashed border-line bg-surface/50 p-4">
          <div className="flex items-center gap-2 text-xs text-muted">
            <Spinner className="h-3.5 w-3.5" />
            正在对比已有知识、检查冲突与新增…
          </div>
        </Card>
      ) : null}

      {summary ? (
        <Card className="p-4">
          {summary.proposals > 0 ? (
            <>
              <p className="text-xs text-ink">
                发现 <span className="font-medium">{summary.proposals}</span> 条需要你确认的知识
                变化。
              </p>
              <ul className="mt-2 space-y-1.5">
                {summary.relations.map((relation) => (
                  <li
                    key={relation.id}
                    className="flex flex-wrap items-center gap-2 rounded-lg border border-line bg-canvas px-3 py-2 text-[11px]"
                  >
                    <Badge tone="accent">{relationshipLabel(relation.relationship)}</Badge>
                    {relation.reason ? (
                      <span className="truncate text-muted">{relation.reason}</span>
                    ) : null}
                  </li>
                ))}
              </ul>
              <Link
                to="/review"
                className="mt-3 inline-block text-[11px] text-accent hover:underline"
              >
                去处理 →
              </Link>
            </>
          ) : (
            <p className="text-xs text-muted">没有发现与已有知识冲突的内容。</p>
          )}
        </Card>
      ) : null}
    </section>
  );
}
