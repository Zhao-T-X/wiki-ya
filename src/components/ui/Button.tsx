import type { ButtonHTMLAttributes, ReactNode } from 'react';

import { Spinner } from '@/components/ui/Spinner';
import { cn } from '@/lib/cn';

export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger';
export type ButtonSize = 'sm' | 'md';

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  loading?: boolean;
  children?: ReactNode;
}

const VARIANTS: Record<ButtonVariant, string> = {
  primary: 'border border-transparent bg-accent text-canvas hover:bg-accent/90',
  secondary: 'border border-line bg-elevated text-ink hover:border-muted/40 hover:bg-elevated/70',
  ghost: 'border border-transparent bg-transparent text-muted hover:bg-elevated hover:text-ink',
  danger: 'border border-danger/40 bg-transparent text-danger hover:bg-danger/10',
};

const SIZES: Record<ButtonSize, string> = {
  sm: 'h-8 gap-1.5 rounded-lg px-3 text-xs',
  md: 'h-9 gap-2 rounded-lg px-3.5 text-sm',
};

export function Button({
  variant = 'secondary',
  size = 'md',
  loading = false,
  className,
  disabled,
  children,
  type = 'button',
  ...rest
}: ButtonProps) {
  return (
    <button
      type={type}
      disabled={disabled || loading}
      className={cn(
        'inline-flex select-none items-center justify-center font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50',
        VARIANTS[variant],
        SIZES[size],
        className,
      )}
      {...rest}
    >
      {loading ? <Spinner className="h-3.5 w-3.5" /> : null}
      {children}
    </button>
  );
}
