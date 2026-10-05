import { Slot } from '@radix-ui/react-slot';
import { cva, type VariantProps } from 'class-variance-authority';
import type { ButtonHTMLAttributes, ReactNode } from 'react';

import { Spinner } from '@/components/ui/Spinner';
import { cn } from '@/lib/cn';

/**
 * 按钮。
 *
 * PR-07 起改用 shadcn 的 cva 变体模式，但刻意保持了两点与上游不同：
 *
 * 1. 文件名保持大写 Button.tsx。shadcn 约定小写 button.tsx，但 macOS 文件系统
 *    大小写不敏感——直接改成小写会**静默覆盖**现有组件（本次踩到了：写入
 *    button.tsx 后原来的 Button.tsx 消失，全项目 import 报错）。沿用大写可以让
 *    50+ 处 @/components/ui/Button 的 import 零改动。
 * 2. 颜色全部映射到本项目语义 token（accent / line / ink / canvas），不引入
 *    shadcn 自带的 CSS variable 体系——那会造成两套颜色真相，暗色必然漂移。
 *
 * 各 variant / size 的类名与改造前逐字一致，保证零视觉回归；
 * xs / lg / icon / asChild 是新增。
 */
const buttonVariants = cva(
  'inline-flex select-none items-center justify-center font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/50',
  {
    variants: {
      variant: {
        primary: 'border border-transparent bg-accent text-canvas hover:bg-accent/90',
        secondary: 'border border-line bg-elevated text-ink hover:border-muted/40 hover:bg-elevated/70',
        ghost: 'border border-transparent bg-transparent text-muted hover:bg-elevated hover:text-ink',
        danger: 'border border-danger/40 bg-transparent text-danger hover:bg-danger/10',
        link: 'border border-transparent bg-transparent text-accent underline-offset-4 hover:underline',
      },
      size: {
        xs: 'h-6 gap-1 rounded-md px-2 text-[11px]',
        sm: 'h-8 gap-1.5 rounded-lg px-3 text-xs',
        md: 'h-9 gap-2 rounded-lg px-3.5 text-sm',
        lg: 'h-11 gap-2 rounded-lg px-6 text-sm',
        icon: 'h-8 w-8 rounded-lg',
      },
    },
    defaultVariants: { variant: 'secondary', size: 'md' },
  },
);

export type ButtonVariant = NonNullable<VariantProps<typeof buttonVariants>['variant']>;
export type ButtonSize = NonNullable<VariantProps<typeof buttonVariants>['size']>;

export interface ButtonProps
  extends ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  /** 展示加载态：内嵌 Spinner 并禁用按钮。 */
  loading?: boolean;
  /** 渲染为子元素（例如 Link），保持按钮样式但语义与跳转正确。 */
  asChild?: boolean;
  /** 右侧附加内容（图标 / 计数）。 */
  trailing?: ReactNode;
  children?: ReactNode;
}

export function Button({
  variant,
  size,
  loading = false,
  asChild = false,
  trailing,
  className,
  disabled,
  children,
  type = 'button',
  ...rest
}: ButtonProps) {
  const Comp = asChild ? Slot : 'button';
  return (
    <Comp
      type={asChild ? undefined : type}
      disabled={disabled || loading}
      className={cn(buttonVariants({ variant, size }), className)}
      {...rest}
    >
      {loading ? <Spinner className="h-3.5 w-3.5" /> : null}
      {children}
      {trailing}
    </Comp>
  );
}

export { buttonVariants };
