import { useNavigate } from 'react-router-dom';

import { ClockIcon } from '@/components/icons';
import { Badge } from '@/components/ui/Badge';
import { Card } from '@/components/ui/Card';
import { Spinner } from '@/components/ui/Spinner';
import { PageHeader } from '@/components/PageHeader';
import { list_timeline } from '@/lib/api';
import { useAsyncData } from '@/lib/hooks';
import type { TimelineItem } from '@/types/ipc';

const KIND_META: Record<string, { label: string; tone: 'ok' | 'accent' | 'warn' | 'neutral' }> = {
  document: { label: '文档', tone: 'neutral' },
  claim: { label: 'Claim', tone: 'accent' },
  relation: { label: '关系', tone: 'warn' },
  research: { label: '研究', tone: 'ok' },
};

function formatAt(at: string): string {
  if (!at) return '';
  const date = new Date(at.replace(' ', 'T') + 'Z');
  if (Number.isNaN(date.getTime())) return at;
  return date.toLocaleString();
}

/**
 * Timeline 时间线（Phase 4 收尾）：把 Document / Claim / 关系 / 研究 的关键时间点
 * 聚合成统一时间轴。数据全部来自 `list_timeline`（真实聚合，无估算）。
 */
export function TimelinePage() {
  const navigate = useNavigate();
  const timeline = useAsyncData(() => list_timeline(), []);

  return (
    <div>
      <PageHeader
        title="Timeline"
        subtitle="知识库的演进时间线：文档捕获、Claim 建立、关系确认与研究任务的真实时间戳。"
      />

      {timeline.loading && !timeline.data ? (
        <div className="flex items-center gap-2 text-xs text-muted">
          <Spinner className="h-3.5 w-3.5" />
          加载时间线…
        </div>
      ) : null}

      {timeline.error ? (
        <Card className="p-5 text-xs text-muted">
          暂无法加载时间线（{timeline.error.message}）。
        </Card>
      ) : null}

      {timeline.data && timeline.data.length === 0 ? (
        <Card className="p-5 text-xs text-muted">
          暂无时间线事件。捕获文档或运行研究后即可在此看到演进记录。
        </Card>
      ) : null}

      {timeline.data && timeline.data.length > 0 ? (
        <Card className="divide-y divide-line">
          {timeline.data.map((item: TimelineItem) => {
            const meta = KIND_META[item.kind] ?? { label: item.kind, tone: 'neutral' as const };
            return (
              <button
                key={`${item.kind}-${item.id}`}
                type="button"
                onClick={() => {
                  if (item.kind === 'document') navigate(`/documents/${item.id}`);
                  else if (item.kind === 'claim') navigate(`/claims/${item.id}`);
                }}
                className="flex w-full items-start gap-3 px-4 py-3 text-left transition-colors hover:bg-elevated/60"
              >
                <span className="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-surface text-muted">
                  <ClockIcon className="h-3.5 w-3.5" />
                </span>
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <Badge tone={meta.tone}>{meta.label}</Badge>
                    <span className="truncate text-sm text-ink">{item.title}</span>
                  </div>
                  {item.detail ? (
                    <p className="mt-0.5 truncate text-[11px] text-muted">{item.detail}</p>
                  ) : null}
                </div>
                <span className="shrink-0 text-[10px] text-muted/70">{formatAt(item.at)}</span>
              </button>
            );
          })}
        </Card>
      ) : null}
    </div>
  );
}
