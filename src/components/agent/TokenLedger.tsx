import { cn } from '@/lib/cn';
import { formatNumber } from '@/lib/format';
import type { TokenUsage } from '@/types/ipc';

export interface TokenLedgerProps {
  usage?: TokenUsage;
  /** 成本估算（美元）；模型未知时后端给 null，此时不显示成本。 */
  costUsd?: number;
  className?: string;
}

/**
 * 真实 token 账本。
 *
 * 数据全部来自 provider 上报的真实 usage（PR-04/PR-05），不是估算；但**任何
 * 数字缺失都只显示占位符，绝不显示 0** —— 后端没上报就说"未知"，把未知说成
 * 0 是这套账本最忌讳的事。
 */
export function TokenLedger({ usage, costUsd, className }: TokenLedgerProps) {
  if (!usage) {
    return (
      <p className={cn('text-meta text-muted', className)}>
        本次运行没有上报用量（本地推理不计费，或该端点未返回 usage）。
      </p>
    );
  }

  const item = (label: string, value: number) => (
    <div key={label} className="flex items-baseline gap-1.5">
      <span className="text-[10px] uppercase tracking-wider text-muted">{label}</span>
      <span className="font-mono text-meta text-ink">{formatNumber(value)}</span>
    </div>
  );

  return (
    <div className={cn('flex flex-wrap items-center gap-x-4 gap-y-1.5', className)}>
      {item('输入', usage.inputTokens)}
      {item('输出', usage.outputTokens)}
      {usage.embeddingTokens > 0 ? item('向量化', usage.embeddingTokens) : null}
      {usage.retries > 0 ? item('重试', usage.retries) : null}
      {costUsd !== undefined ? (
        <div className="flex items-baseline gap-1.5">
          <span className="text-[10px] uppercase tracking-wider text-muted">成本</span>
          <span className="font-mono text-meta text-ink">${costUsd.toFixed(4)}</span>
        </div>
      ) : null}
    </div>
  );
}
