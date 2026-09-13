import { useEffect, type ReactNode } from 'react';
import { createPortal } from 'react-dom';

import { CloseIcon } from '@/components/icons';
import { cn } from '@/lib/cn';

export interface ModalProps {
  open: boolean;
  onClose: () => void;
  title: string;
  description?: ReactNode;
  footer?: ReactNode;
  /** 内容区最大宽度类名。 */
  widthClassName?: string;
  children: ReactNode;
}

export function Modal({
  open,
  onClose,
  title,
  description,
  footer,
  widthClassName = 'max-w-2xl',
  children,
}: ModalProps) {
  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [open, onClose]);

  if (!open) return null;

  return createPortal(
    <div
      className="fixed inset-0 z-50 overflow-y-auto bg-black/60 p-6 backdrop-blur-sm"
      onMouseDown={onClose}
      role="presentation"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className={cn(
          'mx-auto mt-10 w-full overflow-hidden rounded-xl border border-line bg-surface shadow-2xl shadow-black/40',
          widthClassName,
        )}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="flex items-start justify-between gap-4 border-b border-line px-5 py-4">
          <div>
            <h2 className="text-sm font-semibold text-ink">{title}</h2>
            {description ? <p className="mt-1 text-xs text-muted">{description}</p> : null}
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="关闭"
            className="rounded-md p-1 text-muted transition-colors hover:bg-elevated hover:text-ink"
          >
            <CloseIcon className="h-4 w-4" />
          </button>
        </header>
        <div className="px-5 py-4">{children}</div>
        {footer ? <footer className="flex justify-end gap-2 border-t border-line px-5 py-3">{footer}</footer> : null}
      </div>
    </div>,
    document.body,
  );
}
