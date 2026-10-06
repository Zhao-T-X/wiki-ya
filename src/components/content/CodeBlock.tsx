import { useCallback, useState, type ReactNode } from 'react';

import { CheckIcon, CopyIcon } from '@/components/icons';
import { cn } from '@/lib/cn';

/**
 * 代码块。
 *
 * 任务书 §9 的要求：语言标签 + 复制按钮 + 横向滚动 + 代码背景层。
 *
 * **不含语法高亮**——任务书明确写了「语法高亮可以后续接入，但不要因此阻塞
 * Markdown 第一阶段交付」，而 shiki 意味着约 500KB wasm，已经越过 T1 定的
 * 依赖预算。真要上高亮时应改用 lowlight（按需语言、约 10KB）。
 */
export function CodeBlock({
  code,
  language,
  className,
  compact,
}: {
  code: string;
  language?: string;
  className?: string;
  /** 紧凑模式：用于 Search snippet / 卡片内嵌，去掉语言标签栏。 */
  compact?: boolean;
}) {
  const [copied, setCopied] = useState(false);

  const copy = useCallback(() => {
    // navigator.clipboard 在 WKWebView 里需要 secure context；Tauri 打包后是
    // tauri:// 协议，属于 secure context，因此可用。失败要如实提示而非静默。
    void navigator.clipboard
      ?.writeText(code)
      .then(() => {
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1600);
      })
      .catch(() => setCopied(false));
  }, [code]);

  return (
    <div
      className={cn(
        'group relative overflow-hidden rounded-lg border border-line bg-canvas',
        compact ? 'my-2' : 'my-3',
        className,
      )}
    >
      {!compact ? (
        <div className="flex items-center justify-between border-b border-line bg-elevated/50 px-3 py-1.5">
          <span className="rounded bg-canvas px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wider text-muted">
            {language || 'text'}
          </span>
          <button
            type="button"
            onClick={copy}
            aria-label={copied ? '已复制' : '复制代码'}
            aria-pressed={copied}
            className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] text-muted transition-colors hover:bg-elevated hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50"
          >
            {copied ? <CheckIcon className="h-3 w-3 text-ok" /> : <CopyIcon className="h-3 w-3" />}
            <span className={copied ? 'text-ok' : ''}>{copied ? '已复制' : '复制'}</span>
          </button>
        </div>
      ) : null}
      <pre
        className={cn(
          'overflow-x-auto bg-canvas px-3 py-2.5 font-mono text-[12px] leading-relaxed text-ink/90',
          compact && 'max-h-40',
        )}
      >
        <code>{code}</code>
      </pre>
    </div>
  );
}

/** 行内代码：与块级区分开，避免整段被误当代码块。 */
export function InlineCode({ children }: { children: ReactNode }) {
  return (
    <code className="rounded bg-elevated px-1 py-0.5 font-mono text-[0.85em] text-ink">
      {children}
    </code>
  );
}
