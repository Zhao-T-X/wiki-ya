export type ClassValue = string | number | false | null | undefined;

/**
 * 极小的 className 合并工具：过滤假值后以空格拼接。
 * 不引入 clsx / tailwind-merge（禁止新增依赖）。
 */
export function cn(...values: ClassValue[]): string {
  return values.filter((value): value is string | number => Boolean(value)).join(' ');
}
