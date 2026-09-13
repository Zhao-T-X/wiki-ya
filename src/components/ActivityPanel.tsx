import { useEffect, useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';

import { Badge } from '@/components/ui/Badge';
import { Card } from '@/components/ui/Card';
import { Spinner } from '@/components/ui/Spinner';
import { list_documents, list_extraction_runs } from '@/lib/api';
import { isTerminal, STAGE_LABEL, STATUS_LABEL, STATUS_TONE } from '@/lib/extraction';
import { useExtractionEvents } from '@/lib/useExtractionEvents';
import type { DocumentSummary, ExtractionRunDto } from '@/types/ipc';

/**
 * Activity —— 抽取运行历史（EXTRACTION-001）。
 *
 * 列出最近的 Extraction Run，实时反映后台任务的进度 / 状态；
 * 点击可跳到对应文档。任务持久化在 `extraction_runs` 表，
 * 因此页面关了 / 应用重启后再回来，历史依然在。
 */
export function ActivityPanel() {
  const navigate = useNavigate();
  const [runs, setRuns] = useState<ExtractionRunDto[]>([]);
  const [titles, setTitles] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(true);

  function refresh() {
    list_extraction_runs({ limit: 12 })
      .then(setRuns)
      .catch(() => {})
      .finally(() => setLoading(false));
  }

  useEffect(() => {
    refresh();
    list_documents({ limit: 100 })
      .then((docs: DocumentSummary[]) => {
        const map: Record<string, string> = {};
        for (const doc of docs) map[doc.id] = doc.title;
        setTitles(map);
      })
      .catch(() => {});
  }, []);

  // 任意抽取事件都触发一次刷新（任何 Run 进度/状态变化都可见）。
  useExtractionEvents(null, () => refresh());

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
        {runs.map((run) => {
          const title = titles[run.documentId] ?? run.documentId.slice(0, 8);
          const running = !isTerminal(run.status);
          const tone = STATUS_TONE[run.status] ?? 'neutral';
          return (
            <Card key={run.id} className="p-3">
              <button
                type="button"
                className="flex w-full items-start justify-between gap-3 text-left"
                onClick={() => navigate(`/documents/${run.documentId}`)}
              >
                <div className="min-w-0 space-y-1">
                  <div className="flex flex-wrap items-center gap-1.5">
                    <Badge tone={tone}>{STATUS_LABEL[run.status] ?? run.status}</Badge>
                    <span className="truncate text-sm text-ink">{title}</span>
                  </div>
                  <p className="text-[11px] leading-relaxed text-muted">
                    {running && run.stage
                      ? `${STAGE_LABEL[run.stage] ?? run.stage}`
                      : null}
                    {run.totalChunks > 0
                      ? ` · ${run.processedChunks}/${run.totalChunks} 块`
                      : null}
                    {!running && run.candidatesFound > 0
                      ? ` · ${run.candidatesFound} 候选`
                      : null}
                    {!running && run.changesFound > 0 ? ` · ${run.changesFound} 变更` : null}
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
            </Card>
          );
        })}
      </div>
    </section>
  );
}
