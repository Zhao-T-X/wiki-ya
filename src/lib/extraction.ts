/** Extraction Run 的状态 / 阶段展示标签与终态判定（EXTRACTION-001）。 */

export const TERMINAL = new Set(['completed', 'failed', 'cancelled', 'interrupted']);

export function isTerminal(status: string | undefined): boolean {
  return !!status && TERMINAL.has(status);
}

export const STAGE_LABEL: Record<string, string> = {
  preparing: '准备中',
  chunking: '分块',
  extracting: '抽取中',
  validating: '校验中',
  comparing: '比对中',
  finalizing: '收尾',
};

export const STATUS_LABEL: Record<string, string> = {
  queued: '排队中',
  running: '运行中',
  completed: '已完成',
  failed: '失败',
  cancelled: '已取消',
  interrupted: '已中断',
};

export const STATUS_TONE: Record<string, 'accent' | 'ok' | 'warn' | 'neutral'> = {
  queued: 'neutral',
  running: 'accent',
  completed: 'ok',
  failed: 'warn',
  cancelled: 'neutral',
  interrupted: 'neutral',
};
