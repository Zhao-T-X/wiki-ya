import { useRef, useState } from 'react';

import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { ErrorNotice } from '@/components/ErrorNotice';
import { Spinner } from '@/components/ui/Spinner';
import { SparkIcon } from '@/components/icons';
import {
  analyze_document,
  create_claim,
  extract_claims,
  WikiError,
} from '@/lib/api';
import type { CreateClaimInput, ExtractedClaim, ExtractionReport } from '@/types/ipc';

interface ExtractionPanelProps {
  documentId: string;
  /** 抽取并落库后回调（用于刷新文档详情）。 */
  onClaimsAccepted: () => void;
}

function itemKey(item: ExtractedClaim): string {
  return `${item.subject}|${item.predicate}|${item.sentence ?? ''}`;
}

/**
 * AI 抽取面板（Phase 5）。
 *
 * 只预览、不擅自落库：用户逐条或批量「接受」后，才调用既有的 `create_claim`
 * + `analyze_document`（复用 Review / Evolution 流程），由用户最终决定。
 *
 * 诚实优先：未配置 `WIKIYA_API_KEY` 时后端返回 `enabled: false` 与说明，
 * 这里原样展示「AI 未启用」，绝不伪造任何抽取结果（PRD「AI suggests, user decides」）。
 */
export function ExtractionPanel({ documentId, onClaimsAccepted }: ExtractionPanelProps) {
  const [report, setReport] = useState<ExtractionReport | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<WikiError | null>(null);
  const [accepted, setAccepted] = useState<string[]>([]);
  const [accepting, setAccepting] = useState(false);
  // 在途请求用 ref 同步记录：state 异步更新，光靠它无法阻止同一 tick 内的重复提交。
  const pendingRef = useRef<Set<string>>(new Set());
  const [pendingKeys, setPendingKeys] = useState<string[]>([]);
  const acceptingRef = useRef(false);

  async function runExtraction() {
    setLoading(true);
    setError(null);
    setAccepted([]);
    setReport(null);
    try {
      const result = await extract_claims({ id: documentId });
      setReport(result);
    } catch (cause: unknown) {
      setError(cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause)));
    } finally {
      setLoading(false);
    }
  }

  async function acceptItem(item: ExtractedClaim) {
    const key = itemKey(item);
    // 已接受或在途 → 忽略。否则双击 / 批量进行中再点单条会写入重复 Claim。
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
      // 一次性把新 Claim 与库内既有 Claim 做演化分析（duplicate / contradicts 等）。
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

  return (
    <section>
      <div className="mb-3 flex items-center justify-between">
        <h3 className="text-[11px] font-semibold uppercase tracking-wider text-muted">AI 抽取</h3>
        <Button size="sm" variant="primary" loading={loading} onClick={runExtraction}>
          <SparkIcon className="h-3.5 w-3.5" />
          抽取 Claim
        </Button>
      </div>

      {error ? <ErrorNotice error={error} className="mb-3" /> : null}

      {loading ? (
        <div className="flex items-center gap-2 py-4 text-xs text-muted">
          <Spinner className="h-3.5 w-3.5" />
          正在调用 AI 抽取…
        </div>
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
              </span>
              <Button
                size="sm"
                variant="ghost"
                loading={accepting}
                onClick={acceptAll}
                disabled={loading || accepting || report.extracted.every((item) => acceptedSet.has(itemKey(item)))}
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
