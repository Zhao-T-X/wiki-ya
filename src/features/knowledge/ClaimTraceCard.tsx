import { Link } from 'react-router-dom';

import { Badge } from '@/components/ui/Badge';
import { Card } from '@/components/ui/Card';
import { ErrorNotice } from '@/components/ErrorNotice';
import { Spinner } from '@/components/ui/Spinner';
import { get_claim_trace } from '@/lib/api';
import { formatDateTime } from '@/lib/format';
import { relationshipLabel } from '@/lib/status';
import { useAsyncData } from '@/lib/hooks';

/**
 * Claim 溯源卡片（M8：Knowledge Trace）。
 *
 * 从当前知识反向一路追溯到源头：
 *   Claim → 演化关系 → 候选（Candidate）→ 抽取 Run（skill@version）→ 原文/切片。
 * 数据来自 `get_claim_trace`（一次聚合查询）。
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

  return (
    <div className="space-y-2">
      {/* 1) 来源：Run + Skill 版本 */}
      {run ? (
        <Card className="p-3 text-[11px]">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-muted">产生自</span>
            <Badge tone="accent">{run.actor || run.runType}</Badge>
            <span className="text-muted">
              {run.runType === 'extraction'
                ? '抽取 Run'
                : `${run.runType} Run`}
              {' · '}
              {formatDateTime(run.startedAt)}
            </span>
            <Badge tone={run.status === 'completed' ? 'ok' : 'warn'}>
              {run.status}
            </Badge>
          </div>
        </Card>
      ) : null}

      {/* 2) 原文：文档 / 切片 */}
      {evidences.length > 0 ? (
        <Card className="p-3 text-[11px]">
          <div className="space-y-1">
            {evidences.map((evidence) => (
              <div key={evidence.evidenceId} className="flex flex-wrap items-center gap-2">
                <span className="text-muted">来源</span>
                <Link
                  to={`/documents/${evidence.documentId}`}
                  className="text-accent hover:underline"
                >
                  {evidence.documentTitle || evidence.documentId.slice(0, 8)}
                </Link>
                {evidence.chunkIndex !== null && evidence.chunkIndex !== undefined ? (
                  <span className="text-muted/70">切片 #{evidence.chunkIndex}</span>
                ) : null}
                {evidence.quote ? (
                  <span className="truncate text-muted/70">“{evidence.quote}”</span>
                ) : null}
              </div>
            ))}
          </div>
        </Card>
      ) : null}

      {/* 3) 演化：参与了哪些关系（supersedes / contradicts / …） */}
      {evolutions.length > 0 ? (
        <Card className="p-3 text-[11px]">
          <div className="space-y-1">
            {evolutions.map((evolution) => (
              <div key={evolution.relationId} className="flex flex-wrap items-center gap-2">
                <Badge tone={evolution.status === 'accepted' ? 'ok' : 'neutral'}>
                  {relationshipLabel(evolution.relationship)}
                </Badge>
                <span className="text-muted/70">{evolution.status}</span>
                <span className="ml-auto text-muted/70">
                  {formatDateTime(evolution.createdAt)}
                </span>
              </div>
            ))}
          </div>
        </Card>
      ) : null}

      {/* 4) 无溯源：手工录入的知识 */}
      {!candidate && evidences.length === 0 && evolutions.length === 0 ? (
        <Card className="p-3 text-[11px] text-muted">
          这条知识由手动录入，暂无自动抽取溯源。
        </Card>
      ) : null}
    </div>
  );
}
