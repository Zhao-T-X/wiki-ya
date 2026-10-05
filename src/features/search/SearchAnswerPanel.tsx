import { useState } from 'react';

import { MarkdownContent } from '@/components/content/MarkdownContent';
import { SourceList } from '@/components/agent/SourceCard';
import { TokenLedger } from '@/components/agent/TokenLedger';
import { SparkIcon } from '@/components/icons';
import { Badge } from '@/components/ui/Badge';
import { Card } from '@/components/ui/Card';
import { Collapse } from '@/components/agent/Collapse';
import { EmptyState } from '@/components/ui/EmptyState';
import { Spinner } from '@/components/ui/Spinner';
import { formatCompressionRatio } from '@/lib/format';
import type { AskResponse } from '@/types/ipc';

/**
 * 答案面板（任务书 §16.1）。
 *
 * 定位：**增强层，不是 Search 的主体**。检索结果先出现，用户想要解读时才点
 * 「基于这些结果让 AI 回答」。
 *
 * 三层结构（任务书 §5.2）：
 * - Level 1：答案正文（Markdown 渲染）
 * - Level 1：来源卡片（`[n]` 角标可点，双向联动）
 * - Level 3：token 账本 / 上下文统计（折叠在「运行细节」里）
 */
export function SearchAnswerPanel({
  answer,
  streaming,
  answering,
  error,
  onCitation,
}: {
  answer: AskResponse | null;
  /** 流式中间态文本。 */
  streaming: string;
  answering: boolean;
  error: string | null;
  onCitation: (index: number) => void;
}) {
  const [activeSource, setActiveSource] = useState<number | null>(null);

  if (error) {
    return (
      <Card tone="quiet" className="border-danger/30 p-4 text-sm text-danger">
        {error}
      </Card>
    );
  }

  if (!answering && !answer) {
    return (
      <EmptyState
        title="让 AI 解读"
        description="基于上面的检索结果生成带引用的回答。回答可能出错，每条结论都可以点开来源核对。"
        icon={<SparkIcon className="h-5 w-5" />}
      />
    );
  }

  const jump = (index: number) => {
    setActiveSource(index);
    document.getElementById(`source-${index}`)?.scrollIntoView({
      behavior: 'smooth',
      block: 'center',
    });
    onCitation(index);
  };

  return (
    <div className="space-y-3">
      <Card className="p-5">
        <div className="mb-3 flex items-center justify-between gap-2">
          <h2 className="text-sm font-semibold text-ink">回答</h2>
          {answering ? (
            <Badge tone="accent">
              <Spinner className="mr-1 h-3 w-3" />
              生成中
            </Badge>
          ) : null}
        </div>

        {answering && streaming === '' ? (
          <div className="flex items-center gap-2 text-sm text-muted">
            <Spinner className="h-3.5 w-3.5" />
            正在检索知识库并组织答案…
          </div>
        ) : null}

        {streaming ? (
          <MarkdownContent onCitation={jump} className="animate-pulse">
            {streaming}
          </MarkdownContent>
        ) : null}

        {answer && !answer.enabled ? (
          <div>
            <p className="text-body font-medium text-ink">当前无法回答</p>
            <p className="mt-1 text-secondary leading-relaxed text-muted">
              {answer.note ?? 'AI 未启用或问答链路暂不可用。请在设置里配置 API Key。'}
            </p>
          </div>
        ) : null}

        {answer && answer.enabled ? (
          <>
            <MarkdownContent onCitation={jump}>{answer.answer}</MarkdownContent>
            {answer.note ? (
              <p className="mt-3 text-meta leading-relaxed text-warn">{answer.note}</p>
            ) : null}
          </>
        ) : null}
      </Card>

      {answer && answer.sources.length > 0 ? (
        <Card className="p-5">
          <h2 className="mb-3 text-sm font-semibold text-ink">来源（{answer.sources.length}）</h2>
          <SourceList sources={answer.sources} activeIndex={activeSource} />
        </Card>
      ) : null}

      {/* Level 3 技术信息，默认折叠（任务书 §5.2）。 */}
      {answer ? (
        <Collapse
          title="运行细节"
          hint={answer.agentRunId ? answer.agentRunId.slice(0, 8) : undefined}
        >
          <div className="space-y-2">
            <TokenLedger usage={answer.usage} costUsd={answer.costUsd} />
            {answer.contextStats ? (
              <p className="font-mono text-[10px] leading-relaxed text-muted">
                上下文 {answer.contextStats.loadedTokens}/{answer.contextStats.totalTokens}{' '}
                tokens · {answer.contextStats.itemCount} 条 · 压缩比{' '}
                {formatCompressionRatio(answer.contextStats.compressionRatio)}
                {answer.contextStats.truncated ? ' · 已截断' : ''}
              </p>
            ) : null}
          </div>
        </Collapse>
      ) : null}
    </div>
  );
}
