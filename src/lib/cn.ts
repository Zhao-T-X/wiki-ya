import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

/**
 * className 合并。
 *
 * ## 为什么现在引入了 clsx / tailwind-merge
 *
 * 此前这里是一个 9 行的自建实现，注释写着「不引入 clsx / tailwind-merge
 * （禁止新增依赖）」。PR-07 起该约束解除，原因是组件体系转向 shadcn/ui + Radix：
 * tailwind-merge 能按 Tailwind 语义消解冲突（`px-2 px-3` 取后者、
 * `text-sm text-ink` 两个都留），而简单拼接会把冲突类名同时写进 DOM，
 * 样式结果取决于 CSS 顺序而非意图。
 *
 * clsx 负责条件类名（`isActive && 'active'`），是 shadcn 组件的输入格式。
 *
 * 边界：cn 仍是唯一的 className 出口，组件里不直接写 clsx()。
 */
export type { ClassValue };

export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}
