import type { TextareaHTMLAttributes } from 'react';

import { cn } from '@/lib/cn';

export interface TextareaProps extends TextareaHTMLAttributes<HTMLTextAreaElement> {}

export function Textarea({ className, ...rest }: TextareaProps) {
  return (
    <textarea
      className={cn(
        'w-full resize-y rounded-lg border border-line bg-canvas px-3 py-2.5 text-sm leading-relaxed text-ink outline-none transition-colors placeholder:text-muted/70 focus:border-accent/70 disabled:opacity-50',
        className,
      )}
      {...rest}
    />
  );
}
