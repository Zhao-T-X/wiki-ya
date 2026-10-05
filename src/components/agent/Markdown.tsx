/**
 * 受限 Markdown 渲染器（零依赖）。
 *
 * ## 为什么自己写
 *
 * - 项目铁律禁止新增依赖（`src/lib/cn.ts:5`、`src/components/icons.tsx:4`）；
 * - 全库 `dangerouslySetInnerHTML` 零命中。这里**继续**保持：文本 → 数据结构 →
 *   React 元素，没有任何 HTML 字符串拼接，所以不存在 XSS 面。
 * - 模型回答的实际形态就是「段落 + 列表 + 代码块 + 少量强调」，完整 CommonMark
 *   是过度设计。
 *
 * ## 支持的子集
 *
 * 段落、`#`~`###` 标题、无序/有序列表、``` 代码块、`>` 引用、`---` 分隔线，
 * 行内的 `` `code` ``、`**粗体**`、`*斜体*`、`[文本](链接)`、以及 `[n]` 引用角标。
 *
 * ## 流式友好
 *
 * 打字机过程中代码块 / 列表常常还没闭合——未闭合的代码块按"到结尾为止"处理，
 * 未闭合的列表按已出现项渲染，不会整块消失。
 */

import { type ReactNode } from 'react';

import { cn } from '@/lib/cn';

// ---------------------------------------------------------------------------
// 块级解析
// ---------------------------------------------------------------------------

type Block =
  | { type: 'p'; text: string }
  | { type: 'heading'; level: 1 | 2 | 3; text: string }
  | { type: 'code'; lang: string; text: string }
  | { type: 'ul'; items: string[] }
  | { type: 'ol'; items: string[] }
  | { type: 'quote'; text: string }
  | { type: 'hr' };

const HEADING = /^(#{1,3})\s+(.*)$/;
const FENCE = /^```\s*([A-Za-z0-9_+-]*)\s*$/;
const UL_ITEM = /^[-*]\s+(.*)$/;
const OL_ITEM = /^\d+[.)]\s+(.*)$/;
const QUOTE = /^>\s?(.*)$/;
const HR = /^(-{3,}|\*{3,})$/;

/** 把 Markdown 源码切成块序列。导出以便将来做单测。 */
export function parseBlocks(source: string): Block[] {
  const lines = source.replace(/\r\n/g, '\n').split('\n');
  // `noUncheckedIndexedAccess` 下索引访问是 `string | undefined`；越界读等价于
  // 空行（结束当前块），所以统一收敛到一处而不是每处写 `?? ''`。
  const at = (i: number): string => lines[i] ?? '';
  const blocks: Block[] = [];
  let index = 0;

  while (index < lines.length) {
    const line = at(index);

    if (line.trim() === '') {
      index += 1;
      continue;
    }

    // 代码块：一直吃到闭合围栏；没闭合就吃到结尾（流式中间态）。
    const fence = FENCE.exec(line.trim());
    if (fence) {
      const lang = fence[1] ?? '';
      const body: string[] = [];
      index += 1;
      while (index < lines.length && !FENCE.test(at(index).trim())) {
        body.push(at(index));
        index += 1;
      }
      index += 1; // 跳过闭合围栏（若不存在则正好越界，while 自然结束）
      blocks.push({ type: 'code', lang, text: body.join('\n') });
      continue;
    }

    if (HR.test(line.trim())) {
      blocks.push({ type: 'hr' });
      index += 1;
      continue;
    }

    const heading = HEADING.exec(line);
    if (heading) {
      blocks.push({
        type: 'heading',
        level: (heading[1]?.length ?? 1) as 1 | 2 | 3,
        text: heading[2] ?? '',
      });
      index += 1;
      continue;
    }

    // 引用：连续 `>` 行合并成一段。
    if (QUOTE.test(line)) {
      const body: string[] = [];
      while (index < lines.length) {
        const quoted = QUOTE.exec(at(index));
        if (!quoted) break;
        body.push(quoted[1] ?? '');
        index += 1;
      }
      blocks.push({ type: 'quote', text: body.join('\n') });
      continue;
    }

    // 列表：同样要求"相邻且同类型"，否则两个列表会被并成一个。
    if (UL_ITEM.test(line)) {
      const items: string[] = [];
      while (index < lines.length) {
        const item = UL_ITEM.exec(at(index));
        if (!item) break;
        items.push(item[1] ?? '');
        index += 1;
      }
      blocks.push({ type: 'ul', items });
      continue;
    }

    if (OL_ITEM.test(line)) {
      const items: string[] = [];
      while (index < lines.length) {
        const item = OL_ITEM.exec(at(index));
        if (!item) break;
        items.push(item[1] ?? '');
        index += 1;
      }
      blocks.push({ type: 'ol', items });
      continue;
    }

    // 段落：吃到空行或下一个块级起始。
    const paragraph: string[] = [];
    while (index < lines.length) {
      const current = at(index);
      if (
        current.trim() === '' ||
        FENCE.test(current.trim()) ||
        HEADING.test(current) ||
        UL_ITEM.test(current) ||
        OL_ITEM.test(current) ||
        QUOTE.test(current) ||
        HR.test(current.trim())
      ) {
        break;
      }
      paragraph.push(current);
      index += 1;
    }
    blocks.push({ type: 'p', text: paragraph.join(' ') });
  }

  return blocks;
}

// ---------------------------------------------------------------------------
// 行内解析
// ---------------------------------------------------------------------------

/** 只放行 http/https/mailto，其余（javascript:、data:…）退化成纯文本。 */
function safeHref(raw: string): string | null {
  const href = raw.trim();
  if (/^(https?:|mailto:|\/|#)/i.test(href)) return href;
  return null;
}

// 顺序即优先级：代码 > 粗体 > 斜体 > 链接 > 角标。
const INLINE =
  /`([^`]+)`|\*\*([^*]+)\*\*|\*([^*]+)\*|\[([^\]]+)\]\(([^)\s]+)\)|\[(\d{1,3})\]/g;

/**
 * 解析行内元素。
 *
 * `onCitation` 存在时，纯数字的 `[n]` 渲染成**可点击角标**——这是答案正文与
 * 证据卡片之间的唯一通路（模型按约定用 `[n]` 标注来源）。
 */
export function parseInline(
  text: string,
  onCitation?: (index: number) => void,
  keyPrefix = 'i',
): ReactNode[] {
  const nodes: ReactNode[] = [];
  let cursor = 0;
  let seq = 0;
  INLINE.lastIndex = 0;

  let match = INLINE.exec(text);
  while (match !== null) {
    if (match.index > cursor) {
      nodes.push(text.slice(cursor, match.index));
    }
    const key = `${keyPrefix}-${seq++}`;

    if (match[1] !== undefined) {
      nodes.push(
        <code
          key={key}
          className="rounded bg-elevated px-1 py-0.5 font-mono text-[0.85em] text-ink"
        >
          {match[1]}
        </code>,
      );
    } else if (match[2] !== undefined) {
      nodes.push(
        <strong key={key} className="font-semibold text-ink">
          {match[2]}
        </strong>,
      );
    } else if (match[3] !== undefined) {
      nodes.push(
        <em key={key} className="italic">
          {match[3]}
        </em>,
      );
    } else if (match[4] !== undefined && match[5] !== undefined) {
      const href = safeHref(match[5]);
      if (href) {
        nodes.push(
          <a
            key={key}
            href={href}
            target={href.startsWith('http') ? '_blank' : undefined}
            rel="noreferrer"
            className="text-accent underline decoration-accent/40 underline-offset-2 hover:decoration-accent"
          >
            {match[4]}
          </a>,
        );
      } else {
        // 不可信协议：只显示文本，不给链接。
        nodes.push(match[4]);
      }
    } else if (match[6] !== undefined) {
      const index = Number(match[6]);
      nodes.push(
        onCitation ? (
          <button
            key={key}
            type="button"
            onClick={() => onCitation(index)}
            title={`跳到来源 [${index}]`}
            className="mx-0.5 inline-flex h-4 min-w-4 items-center justify-center rounded border border-accent/40 bg-accent/10 px-1 align-baseline font-mono text-[10px] leading-none text-accent transition-colors hover:bg-accent/20"
          >
            {index}
          </button>
        ) : (
          <span
            key={key}
            className="mx-0.5 inline-flex h-4 min-w-4 items-center justify-center rounded border border-line px-1 align-baseline font-mono text-[10px] leading-none text-muted"
          >
            {index}
          </span>
        ),
      );
    }

    cursor = match.index + match[0].length;
    match = INLINE.exec(text);
  }

  if (cursor < text.length) {
    nodes.push(text.slice(cursor));
  }
  return nodes;
}

// ---------------------------------------------------------------------------
// 组件
// ---------------------------------------------------------------------------

export interface MarkdownProps {
  text: string;
  /** 点击正文里的 `[n]` 角标时触发（用于跳到对应证据卡片）。 */
  onCitation?: (index: number) => void;
  className?: string;
}

export function Markdown({ text, onCitation, className }: MarkdownProps) {
  const blocks = parseBlocks(text ?? '');

  return (
    <div className={cn('space-y-3 text-sm leading-relaxed text-ink/90', className)}>
      {blocks.map((block, i) => {
        switch (block.type) {
          case 'heading': {
            const size =
              block.level === 1
                ? 'text-base font-semibold'
                : block.level === 2
                  ? 'text-sm font-semibold'
                  : 'text-[13px] font-semibold';
            return (
              <p key={i} className={cn('mt-1 text-ink', size)}>
                {parseInline(block.text, onCitation, `h${i}`)}
              </p>
            );
          }

          case 'code':
            return (
              <div
                key={i}
                className="overflow-hidden rounded-lg border border-line bg-elevated"
              >
                {block.lang ? (
                  <div className="border-b border-line px-3 py-1 font-mono text-[10px] uppercase tracking-wider text-muted">
                    {block.lang}
                  </div>
                ) : null}
                <pre className="overflow-x-auto px-3 py-2 font-mono text-[11px] leading-relaxed text-ink/90">
                  <code>{block.text}</code>
                </pre>
              </div>
            );

          case 'ul':
            return (
              <ul key={i} className="list-disc space-y-1 pl-5 marker:text-muted">
                {block.items.map((item, j) => (
                  <li key={j}>{parseInline(item, onCitation, `u${i}-${j}`)}</li>
                ))}
              </ul>
            );

          case 'ol':
            return (
              <ol
                key={i}
                className="list-decimal space-y-1 pl-5 marker:font-mono marker:text-[11px] marker:text-muted"
              >
                {block.items.map((item, j) => (
                  <li key={j}>{parseInline(item, onCitation, `o${i}-${j}`)}</li>
                ))}
              </ol>
            );

          case 'quote':
            return (
              <blockquote
                key={i}
                className="border-l-2 border-accent/40 pl-3 text-muted italic"
              >
                {parseInline(block.text, onCitation, `q${i}`)}
              </blockquote>
            );

          case 'hr':
            return <hr key={i} className="border-line" />;

          case 'p':
          default:
            return (
              <p key={i} className="break-words whitespace-pre-wrap">
                {parseInline(block.text, onCitation, `p${i}`)}
              </p>
            );
        }
      })}
    </div>
  );
}

/** 流式生成中的光标（打字机效果）。 */
export function StreamingCursor() {
  return (
    <span
      aria-hidden
      className="ml-0.5 inline-block h-3.5 w-[2px] translate-y-0.5 animate-pulse bg-accent"
    />
  );
}
