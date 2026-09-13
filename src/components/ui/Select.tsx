import type { ReactNode, SelectHTMLAttributes } from 'react';

import { ChevronDownIcon } from '@/components/icons';
import { cn } from '@/lib/cn';

export interface SelectProps extends SelectHTMLAttributes<HTMLSelectElement> {
  children: ReactNode;
}

export function Select({ className, children, ...rest }: SelectProps) {
  return (
    <div className="relative">
      <select
        className={cn(
          'h-9 w-full cursor-pointer appearance-none rounded-lg border border-line bg-canvas pl-3 pr-9 text-sm text-ink outline-none transition-colors focus:border-accent/70 disabled:cursor-not-allowed disabled:opacity-50',
          className,
        )}
        {...rest}
      >
        {children}
      </select>
      <ChevronDownIcon className="pointer-events-none absolute right-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted" />
    </div>
  );
}
