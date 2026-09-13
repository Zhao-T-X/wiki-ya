import type { InputHTMLAttributes } from 'react';

import { cn } from '@/lib/cn';

export interface InputProps extends InputHTMLAttributes<HTMLInputElement> {}

const BASE =
  'h-9 w-full rounded-lg border border-line bg-canvas px-3 text-sm text-ink outline-none transition-colors placeholder:text-muted/70 focus:border-accent/70 disabled:opacity-50';

export function Input({ className, ...rest }: InputProps) {
  return <input className={cn(BASE, className)} {...rest} />;
}
