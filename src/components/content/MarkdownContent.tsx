import React from 'react';
import ReactMarkdown from 'react-markdown';
import rehypeSanitize from 'rehype-sanitize';
import remarkGfm from 'remark-gfm';

import { CodeBlock, InlineCode } from '@/components/content/CodeBlock';
import { cn } from '@/lib/cn';

/**
 * 统一内容渲染层（任务书 §8）。
 *
 * ## 为什么替换掉上一轮的自研渲染器
 *
 * 上一轮为了守住「禁止新增依赖」自研了 `agent/Markdown.tsx`，覆盖段落/标题/列表/
 * 代码块/引用/行内。它的问题不是写得不好，而是**上限太低**：没有表格、任务列表、
 * 删除线、自动链接、脚注。知识库场景里表格和任务列表都很常见，所以这里换
 * react-markdown。
 *
 * 那个文件里的两样东西被保留：**引用角标 [n]**（知识库特有语义，标准 Markdown
 * 没有）与**未闭合块容错**（流式中间态，由 compact 截断 + pre 处理共同覆盖）。
 *
 * ## 三种模式
 *
 * - `compact`：Search snippet / 卡片摘要。**限制高度、砍掉大块代码**，避免一条
 *   结果把整篇文档渲染出来（任务书 §12.1 的硬要求）。
 * - `full`：文档正文 / Claim / Ask 答案。
 * - `source`：**不做 Markdown 渲染**，原样输出。用于「原始」视图——看到的字符
 *   就是数据库里的内容，这是 provenance 的一部分（任务书 §10）。
 *
 * ## 安全
 *
 * `rehype-sanitize` 在 remark 之后运行，剥掉原始 HTML；链接额外走协议白名单；
 * 全文不出现 `dangerouslySetInnerHTML`。
 */

export type MarkdownMode = 'compact' | 'full' | 'source';

export interface MarkdownContentProps {
  children: string;
  mode?: MarkdownMode;
  /** 点击正文里的 [n] 角标时触发（跳到对应证据卡片）。 */
  onCitation?: (index: number) => void;
  className?: string;
}

/** compact 模式的字符上限：先截断再渲染，比渲染完再裁剪便宜得多。 */
const COMPACT_MAX_CHARS = 400;

export function MarkdownContent({
  children,
  mode = 'full',
  onCitation,
  className,
}: MarkdownContentProps) {
  const source = children ?? '';

  if (mode === 'source') {
    return (
      <pre
        className={cn(
          'overflow-x-auto whitespace-pre-wrap break-words rounded-lg border border-line bg-canvas p-4',
          'font-mono text-[12px] leading-relaxed text-ink/90',
          className,
        )}
      >
        {source}
      </pre>
    );
  }

  const compact = mode === 'compact';
  const text =
    compact && source.length > COMPACT_MAX_CHARS
      ? `${source.slice(0, COMPACT_MAX_CHARS)}…`
      : source;

  if (text.trim() === '') {
    return <p className={cn('text-sm text-muted', className)}>（空内容）</p>;
  }

  return (
    <div
      className={cn(
        'text-ink/90',
        compact ? 'max-h-32 overflow-hidden text-[13px] leading-relaxed' : 'text-reading',
        className,
      )}
    >
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeSanitize]}
        components={buildComponents(compact, onCitation)}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
}

/** 只有 http/https/mailto 可点，其余降级为纯文本。 */
function safeHref(raw: string | undefined): string | null {
  if (!raw) return null;
  const href = raw.trim();
  return /^(https?:|mailto:|\/|#)/i.test(href) ? href : null;
}

type MarkdownProps = React.ComponentPropsWithoutRef<'p'>;

/**
 * 逐类映射 Markdown 元素到本项目视觉（任务书 §8：不是简单套一个 prose）。
 */
function buildComponents(compact: boolean, onCitation?: (index: number) => void) {
  const H = (level: 1 | 2 | 3 | 4) =>
    // h1~h4 的 props 结构一致，用 h1 作为类型载体（'h' 不是合法标签名）。
    function Heading({ children, ...props }: React.ComponentPropsWithoutRef<'h1'>) {
      const Tag = `h${level}` as 'h1';
      const size =
        level <= 2
          ? 'mt-4 mb-1.5 text-base font-semibold leading-snug'
          : 'mt-3 mb-1 text-sm font-semibold leading-snug';
      return (
        <Tag className={cn(size, level > 2 && 'text-ink/90')} {...props}>
          {children}
        </Tag>
      );
    };

  return {
    h1: H(1),
    h2: H(2),
    h3: H(3),
    h4: H(4),

    // 段落是引用角标的落点：模型按约定用 [n] 标注来源。
    p: ({ children, ...props }: MarkdownProps) => (
      <p className="my-2 break-words whitespace-pre-wrap first:mt-0 last:mb-0" {...props}>
        {withCitations(children, onCitation)}
      </p>
    ),

    a: ({ href, children, ...props }: React.ComponentPropsWithoutRef<'a'>) => {
      const safe = safeHref(href);
      if (!safe) return <span {...props}>{children}</span>;
      const external = /^https?:/i.test(safe);
      return (
        <a
          href={safe}
          target={external ? '_blank' : undefined}
          rel={external ? 'noreferrer' : undefined}
          className="text-accent underline decoration-accent/40 underline-offset-2 hover:decoration-accent"
          {...props}
        >
          {children}
        </a>
      );
    },

    strong: ({ children, ...props }: React.ComponentPropsWithoutRef<'strong'>) => (
      <strong className="font-semibold text-ink" {...props}>
        {children}
      </strong>
    ),
    em: ({ children, ...props }: React.ComponentPropsWithoutRef<'em'>) => (
      <em className="italic" {...props}>
        {children}
      </em>
    ),
    del: ({ children, ...props }: React.ComponentPropsWithoutRef<'del'>) => (
      <del className="text-muted line-through" {...props}>
        {children}
      </del>
    ),

    ul: ({ children, ...props }: React.ComponentPropsWithoutRef<'ul'>) => (
      <ul className="my-2 list-disc space-y-1 pl-5 marker:text-muted" {...props}>
        {children}
      </ul>
    ),
    ol: ({ children, ...props }: React.ComponentPropsWithoutRef<'ol'>) => (
      <ol
        className="my-2 list-decimal space-y-1 pl-5 marker:font-mono marker:text-[11px] marker:text-muted"
        {...props}
      >
        {children}
      </ol>
    ),

    blockquote: ({ children, ...props }: React.ComponentPropsWithoutRef<'blockquote'>) => (
      <blockquote className="my-2 border-l-2 border-accent/40 pl-3 text-muted italic" {...props}>
        {children}
      </blockquote>
    ),

    // GFM 任务列表：react-markdown 把 checkbox 作为 input 传进 children。
    input: (props: React.ComponentPropsWithoutRef<'input'>) => (
      <input type="checkbox" readOnly className="mr-1.5 align-middle accent-accent" {...props} />
    ),

    table: ({ children, ...props }: React.ComponentPropsWithoutRef<'table'>) => (
      <div className="my-3 overflow-x-auto">
        <table className="w-full border-collapse text-[13px]" {...props}>
          {children}
        </table>
      </div>
    ),
    thead: ({ children, ...props }: React.ComponentPropsWithoutRef<'thead'>) => (
      <thead className="border-b border-line" {...props}>
        {children}
      </thead>
    ),
    th: ({ children, ...props }: React.ComponentPropsWithoutRef<'th'>) => (
      <th className="px-2.5 py-1.5 text-left font-semibold text-ink" {...props}>
        {children}
      </th>
    ),
    td: ({ children, ...props }: React.ComponentPropsWithoutRef<'td'>) => (
      <td className="border-b border-line/60 px-2.5 py-1.5 align-top" {...props}>
        {children}
      </td>
    ),

    hr: (props: React.ComponentPropsWithoutRef<'hr'>) => <hr className="my-4 border-line" {...props} />,

    code: ({ children, className, ...props }: React.ComponentPropsWithoutRef<'code'>) => {
      const text = String(children ?? '');
      // react-markdown 对块级代码会传 className="language-xxx"。
      const language = /language-(\w+)/.exec(className ?? '')?.[1];
      if (language || text.includes('\n')) {
        return <CodeBlock code={text.replace(/\n$/, '')} language={language} compact={compact} />;
      }
      return <InlineCode {...props}>{children}</InlineCode>;
    },

    // 块级代码：交给上面的 code 渲染成 CodeBlock，这里只把 pre 拆掉，
    // 否则会形成 <pre><CodeBlock> 的双层容器。
    pre: ({ children }: React.ComponentPropsWithoutRef<'pre'>) => <>{children}</>,
  };
}

/**
 * 把子节点里的纯文本 `[n]` 换成可点击角标。
 *
 * 只处理字符串与数组两种情况（Markdown 段落的子节点基本就是这两种），遇到
 * 元素类型（链接、代码等）原样保留——不拆开已经成形的元素。
 */
function withCitations(
  children: React.ReactNode,
  onCitation?: (index: number) => void,
): React.ReactNode {
  if (!onCitation) return children;

  const walk = (node: React.ReactNode, keyHint: string): React.ReactNode => {
    if (typeof node === 'string') {
      const parts = node.split(/(\[\d{1,3}\])/g);
      if (parts.length === 1) return node;
      return parts.map((part, i) => {
        const matched = /^\[(\d{1,3})\]$/.exec(part);
        if (!matched) return part;
        const index = Number(matched[1]);
        return (
          <button
            key={`${keyHint}-${i}`}
            type="button"
            onClick={() => onCitation(index)}
            title={`跳到来源 [${index}]`}
            className="mx-0.5 inline-flex h-4 min-w-4 items-center justify-center rounded border border-accent/40 bg-accent/10 px-1 align-baseline font-mono text-[10px] leading-none text-accent transition-colors hover:bg-accent/20"
          >
            {index}
          </button>
        );
      });
    }
    if (Array.isArray(node)) {
      return node.map((item, i) => (
        <React.Fragment key={`${keyHint}-${i}`}>{walk(item, `${keyHint}-${i}`)}</React.Fragment>
      ));
    }
    return node;
  };

  return walk(children, 'c');
}
