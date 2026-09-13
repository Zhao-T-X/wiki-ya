/**
 * 展示层格式化工具。只做「显示」，不做业务判断。
 */

const dateTimeFormatter = new Intl.DateTimeFormat('zh-CN', {
  year: 'numeric',
  month: '2-digit',
  day: '2-digit',
  hour: '2-digit',
  minute: '2-digit',
});

const dateFormatter = new Intl.DateTimeFormat('zh-CN', {
  year: 'numeric',
  month: '2-digit',
  day: '2-digit',
});

function parse(iso: string | null | undefined): Date | null {
  if (!iso) return null;
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? null : date;
}

/** 2026-09-13 10:24 */
export function formatDateTime(iso: string | null | undefined): string {
  const date = parse(iso);
  return date ? dateTimeFormatter.format(date) : '—';
}

/** 2026-09-13 */
export function formatDate(iso: string | null | undefined): string {
  const date = parse(iso);
  return date ? dateFormatter.format(date) : '—';
}

/** 年份，用于 Timeline 分组；无法解析返回 null。 */
export function yearOf(iso: string | null | undefined): number | null {
  const date = parse(iso);
  return date ? date.getFullYear() : null;
}

/** 相对时间：刚刚 / 5 分钟前 / 3 小时前 / 2 天前 / 具体日期。 */
export function formatRelativeTime(iso: string | null | undefined): string {
  const date = parse(iso);
  if (!date) return '—';
  const diffMs = Date.now() - date.getTime();
  const future = diffMs < 0;
  const abs = Math.abs(diffMs);
  const minute = 60_000;
  const hour = 60 * minute;
  const day = 24 * hour;

  if (abs < minute) return future ? '片刻后' : '刚刚';
  if (abs < hour) {
    const value = Math.floor(abs / minute);
    return future ? `${value} 分钟后` : `${value} 分钟前`;
  }
  if (abs < day) {
    const value = Math.floor(abs / hour);
    return future ? `${value} 小时后` : `${value} 小时前`;
  }
  if (abs < 30 * day) {
    const value = Math.floor(abs / day);
    return future ? `${value} 天后` : `${value} 天前`;
  }
  return formatDate(iso);
}

const numberFormatter = new Intl.NumberFormat('zh-CN');

export function formatNumber(value: number): string {
  return numberFormatter.format(value);
}

/** 1,234 字 */
export function formatChars(count: number): string {
  return `${formatNumber(count)} 字`;
}

/** 分数保留两位小数；用于 search score。 */
export function formatScore(score: number | null | undefined): string {
  if (score === null || score === undefined || Number.isNaN(score)) return '—';
  return score.toFixed(2);
}

/** 置信度：NULL 展示为「—」，避免把「未知」伪装成 0。 */
export function formatConfidence(confidence: number | null | undefined): string {
  if (confidence === null || confidence === undefined || Number.isNaN(confidence)) return '—';
  return confidence.toFixed(2);
}

export function formatPercent(ratio: number | null | undefined): string {
  if (ratio === null || ratio === undefined || Number.isNaN(ratio)) return '—';
  return `${Math.round(ratio * 100)}%`;
}

/** 截断长文本，保留可读边界。 */
export function truncate(text: string, max = 160): string {
  if (text.length <= max) return text;
  return `${text.slice(0, max).trimEnd()}…`;
}

/** 用时展示：<1s 用 ms，否则用 s。 */
export function formatTookMs(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  return `${(ms / 1000).toFixed(2)} s`;
}

/** 谓词展示：`trained_on` → `trained on`。 */
export function humanizePredicate(predicate: string): string {
  return predicate.replace(/_/g, ' ');
}

/**
 * 生成一个用于关联流式事件的 run id。
 *
 * 优先用 `crypto.randomUUID()`，但在部分 WebView / 非安全上下文下它不存在，
 * 直接调用会抛未捕获异常、让表单静默失败。这里做能力检测并回退到
 * 时间戳 + 随机数（仅用于事件关联，不要求密码学强度）。
 */
export function newRunId(): string {
  const randomUUID = globalThis.crypto?.randomUUID;
  if (typeof randomUUID === 'function') {
    return randomUUID.call(globalThis.crypto);
  }
  return `run-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}
