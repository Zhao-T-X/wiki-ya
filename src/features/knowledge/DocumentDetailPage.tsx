import { useState } from 'react';
import { Link, useParams } from 'react-router-dom';

import { ErrorNotice } from '@/components/ErrorNotice';
import { PageHeader } from '@/components/PageHeader';
import { RefreshIcon, SparkIcon } from '@/components/icons';
import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { EmptyState } from '@/components/ui/EmptyState';
import { Spinner } from '@/components/ui/Spinner';
import { ClaimList } from '@/features/knowledge/ClaimList';
import { ExtractionPanel } from '@/features/knowledge/ExtractionPanel';
import { analyze_document, get_document, reindex_document, WikiError } from '@/lib/api';
import { formatChars, formatDateTime, truncate } from '@/lib/format';
import { useAsyncData } from '@/lib/hooks';

export function DocumentDetailPage() {
  const { documentId } = useParams<{ documentId: string }>();

  const detail = useAsyncData(
    () => (documentId ? get_document({ id: documentId }) : Promise.resolve(null)),
    [documentId],
    Boolean(documentId),
  );

  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [actionError, setActionError] = useState<WikiError | null>(null);

  async function runAction(action: 'reindex' | 'analyze') {
    if (!documentId) return;
    setBusy(true);
    setNotice(null);
    setActionError(null);
    try {
      if (action === 'reindex') {
        const doc = await reindex_document({ id: documentId });
        setNotice(`已重建索引：${doc.chunkCount} 个片段，原文未改动。`);
      } else {
        const report = await analyze_document({ documentId });
        setNotice(
          `确定性分析完成：扫描 ${report.claimsScanned} 条 Claim，写入 ${report.relationsWritten} 条演化关系。`,
        );
      }
      detail.reload();
    } catch (cause: unknown) {
      setActionError(cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause)));
    } finally {
      setBusy(false);
    }
  }

  const data = detail.data;

  return (
    <div>
      <PageHeader
        title={data ? data.document.title : '文档详情'}
        subtitle={
          <Link to="/inbox" className="text-accent hover:underline">
            返回 Inbox
          </Link>
        }
        actions={
          <>
            <Button size="sm" loading={busy} onClick={() => runAction('reindex')}>
              <RefreshIcon className="h-3.5 w-3.5" />
              重建索引
            </Button>
            <Button size="sm" loading={busy} onClick={() => runAction('analyze')}>
              <SparkIcon className="h-3.5 w-3.5" />
              确定性分析
            </Button>
          </>
        }
      />

      {notice ? (
        <p className="mb-4 rounded-lg border border-ok/30 bg-ok/10 px-3 py-2 text-xs text-ok">{notice}</p>
      ) : null}
      {actionError ? <ErrorNotice error={actionError} className="mb-4" /> : null}

      {detail.loading && !data ? (
        <div className="flex items-center gap-2 py-6 text-xs text-muted">
          <Spinner className="h-3.5 w-3.5" />
          加载文档…
        </div>
      ) : null}

      {detail.error ? <ErrorNotice error={detail.error} /> : null}

      {!detail.loading && !detail.error && !data ? (
        <EmptyState title="未找到该文档" description="它可能不存在，或已被移除。" />
      ) : null}

      {data ? (
        <div className="space-y-6">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-muted">
            <Badge tone="accent">{data.document.sourceType}</Badge>
            <span>{data.document.chunkCount} 个片段</span>
            <span className="text-line">·</span>
            <span>{formatChars(data.document.charCount)}</span>
            <span className="text-line">·</span>
            <span>创建于 {formatDateTime(data.document.createdAt)}</span>
            <span className="text-line">·</span>
            <span className="font-mono" title={data.document.contentHash}>
              {truncate(data.document.contentHash, 20)}
            </span>
          </div>

          <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_320px]">
            <section>
              <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-muted">原文（不可变）</h3>
              <Card className="max-h-[520px] overflow-y-auto p-4">
                <pre className="whitespace-pre-wrap break-words font-mono text-xs leading-relaxed text-ink/90">
                  {data.content}
                </pre>
              </Card>
            </section>

            <section className="space-y-3">
              <h3 className="text-[11px] font-semibold uppercase tracking-wider text-muted">
                Chunks（{data.chunks.length}）
              </h3>
              <ul className="max-h-[520px] space-y-2 overflow-y-auto pr-1">
                {data.chunks.map((chunk) => (
                  <li key={chunk.id} className="rounded-lg border border-line bg-surface p-3">
                    <div className="flex items-center justify-between text-[10px] text-muted">
                      <span className="font-mono">#{chunk.chunkIndex}</span>
                      <span className="font-mono">
                        {chunk.startOffset}–{chunk.endOffset}
                      </span>
                    </div>
                    <p className="mt-1.5 text-[11px] leading-relaxed text-muted">
                      {truncate(chunk.content, 120)}
                    </p>
                    <p className="mt-1 text-[10px] text-muted/70">{formatChars(chunk.charCount)}</p>
                  </li>
                ))}
              </ul>
            </section>
          </div>

          <section>
            <h3 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-muted">
              Claims（{data.claims.length}）
            </h3>
            <ClaimList
              claims={data.claims}
              emptyText="该文档暂无 Claim。AI 抽取未启用时，可在 Knowledge 中手动录入。"
            />
          </section>

          <ExtractionPanel
            documentId={data.document.id}
            onClaimsAccepted={() => detail.reload()}
          />
        </div>
      ) : null}
    </div>
  );
}
