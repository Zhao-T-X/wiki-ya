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

/**
 * compact 模式的字符预算：先做**结构化摘要**再渲染，而不是对原始 Markdown 字符串
 * 直接 `slice`（任务书 PR-07.1 T2）。直接截断会在代码 fence 中间或表格中间切断，
 * 渲染出半截 ```` ``` ```` 或残缺表格。这里按「块边界」取舍：fenced code 整段替换
 * 为占位、其余按段落/列表/表格整块累加，超预算就停在块边界，保证渲染结果始终是
 * 合法 Markdown，不会出现半个表格或没闭合的 code fence。
 */
const COMPACT_MAX_CHARS = 420;

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

  if (compact) {
    const { text, truncated } = prepareCompact(source, COMPACT_MAX_CHARS);
    if (text.trim() === '') {
      return <p className={cn('text-sm text-muted', className)}>（空内容）</p>;
    }
    return (
      <div className={cn('max-h-36 overflow-hidden text-secondary leading-relaxed', className)}>
        <ReactMarkdown
          remarkPlugins={[remarkGfm]}
          rehypePlugins={[rehypeSanitize]}
          components={buildComponents(true, onCitation)}
        >
          {truncated ? `${text}…` : text}
        </ReactMarkdown>
      </div>
    );
  }

  if (source.trim() === '') {
    return <p className={cn('text-sm text-muted', className)}>（空内容）</p>;
  }

  return (
    <div className={cn('text-reading', className)}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeSanitize]}
        components={buildComponents(false, onCitation)}
      >
        {source}
      </ReactMarkdown>
    </div>
  );
}

/**
 * 把原始 Markdown 压成紧凑摘要（任务书 PR-07.1 T2）。
 *
 * 思路：先按空行切成「块」（段落 / 列表 / 表格 / 引用各自成块），fenced code 整段
 * 替换成一个占位块（compact 不渲染代码）；再从前往后整块累加，超过预算就停在块边界。
 * 因为表格、列表、引用都是连续非空行构成的整块，所以要么完整保留、要么整块跳过，
 * 绝不会在表格或代码中间切断。
 */
function prepareCompact(src: string, maxChars: number): { text: string; truncated: boolean } {
  const lines = src.split('\n');
  const blocks: string[] = [];
  let cur: string[] = [];
  let inFence = false;

  const flush = () => {
    if (cur.length > 0) {
      blocks.push(cur.join('\n'));
      cur = [];
    }
  };

  for (const line of lines) {
    const trimmed = line.trim();
    if (trimmed.startsWith('```')) {
      flush();
      if (!inFence) {
        blocks.push('_代码块已省略_');
        inFence = true;
      } else {
        inFence = false;
      }
      continue;
    }
    if (inFence) continue;
    if (trimmed === '') {
      flush();
      continue;
    }
    cur.push(line);
  }
  flush();

  let out = '';
  let used = 0;
  let truncated = false;
  for (const block of blocks) {
    if (out !== '' && used + block.length + 2 > maxChars) {
      truncated = true;
      break;
    }
    out += (out === '' ? '' : '\n\n') + block;
    used += block.length + 2;
  }
  // 第一个块就超长：仍然至少展示它，避免 compact 渲染成空白。
  if (out === '' && blocks.length > 0) {
    out = blocks[0] ?? '';
    truncated = true;
  }
  return { text: out, truncated };
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
    // 任务书 PR-07.1 T1：full 模式拉满层级节奏（h1 20px → 正文 15px → 元 11px），
    // compact 模式压低标题，避免「卡片里的 Markdown」喧宾夺主。
    function Heading({ children, ...props }: React.ComponentPropsWithoutRef<'h1'>) {
      const Tag = `h${level}` as 'h1';
      const size = compact
        ? level <= 2
          ? 'mt-3 mb-1 text-sm font-semibold leading-snug text-ink'
          : 'mt-2 mb-0.5 text-secondary font-semibold leading-snug text-ink/90'
        : level === 1
          ? 'mt-7 mb-3 text-xl font-semibold leading-snug text-ink'
          : level === 2
            ? 'mt-6 mb-2.5 text-lg font-semibold leading-snug text-ink'
            : level === 3
              ? 'mt-5 mb-2 text-base font-semibold leading-snug text-ink'
              : 'mt-4 mb-1.5 text-sm font-semibold leading-snug text-ink';
      return (
        <Tag className={cn(size)} {...props}>
          {children}
        </Tag>
      );
    };

  return {
    h1: H(1),
    h2: H(2),
    h3: H(3),
    h4: H(4),

    // 段落是引用角标的落点：模型按约定用 [n] 标注来源。任务书 PR-07.1 T1：放宽段间距。
    p: ({ children, ...props }: MarkdownProps) => (
      <p className="my-2.5 break-words whitespace-pre-wrap first:mt-0 last:mb-0" {...props}>
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
      <ul className="my-3 list-disc space-y-1.5 pl-5 marker:text-muted" {...props}>
        {children}
      </ul>
    ),
    ol: ({ children, ...props }: React.ComponentPropsWithoutRef<'ol'>) => (
      <ol
        className="my-3 list-decimal space-y-1.5 pl-5 marker:font-mono marker:text-meta marker:text-muted"
        {...props}
      >
        {children}
      </ol>
    ),

    blockquote: ({ children, ...props }: React.ComponentPropsWithoutRef<'blockquote'>) => (
      <blockquote className="my-3 border-l-2 border-accent/40 pl-4 text-muted italic" {...props}>
        {children}
      </blockquote>
    ),

    // GFM 任务列表：react-markdown 把 checkbox 作为 input 传进 children。
    input: (props: React.ComponentPropsWithoutRef<'input'>) => (
      <input type="checkbox" readOnly className="mr-1.5 align-middle accent-accent" {...props} />
    ),

    table: ({ children, ...props }: React.ComponentPropsWithoutRef<'table'>) => (
      <div className="my-4 overflow-x-auto">
        <table className="w-full border-collapse text-secondary" {...props}>
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

    hr: (props: React.ComponentPropsWithoutRef<'hr'>) => <hr className="my-6 border-line" {...props} />,

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
