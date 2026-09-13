import type { ReactNode } from 'react';

import { cn } from '@/lib/cn';

export interface CardProps {
  className?: string;
  children: ReactNode;
}

export function Card({ className, children }: CardProps) {
  return <div className={cn('rounded-xl border border-line bg-surface', className)}>{children}</div>;
}
