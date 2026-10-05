import { EvidenceBlock } from '@/components/content/EvidenceBlock';
import { AlertIcon } from '@/components/icons';
import { Card } from '@/components/ui/Card';

/**
 * 证据段（任务书 §21.3）。
 *
 * 明确区分「有引文」与「无引文」两种状态：确定性规则推断出的关系**本来就没有
 * 引文**，那不是缺陷，但必须如实说出来，不能留一块空白让人以为漏了内容。
 */
export function ReviewEvidence({ quote }: { quote: string | null }) {
  if (!quote) {
    return (
      <Card tone="quiet" className="flex items-start gap-2 p-3">
        <AlertIcon className="mt-0.5 h-3.5 w-3.5 shrink-0 text-warn" />
        <div>
          <p className="text-secondary font-medium text-ink">没有直接引文</p>
          <p className="mt-0.5 text-meta leading-relaxed text-muted">
            这条判定来自确定性规则（主语与谓语相同、对象不同），不是从原文摘出来的。
            规则本身是可靠的，但你无法据此核对原文。
          </p>
        </div>
      </Card>
    );
  }

  return (
    <EvidenceBlock
      quote={quote}
      supportLevel="directly"
      // 关系判定的引文来自 sourceClaim 的证据，溯源入口指向新知识一侧。
      sourceKind="claim"
      className="mt-0"
    />
  );
}
