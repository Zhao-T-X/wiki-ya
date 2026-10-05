import * as DialogPrimitive from '@radix-ui/react-dialog';
import type { ComponentPropsWithoutRef } from 'react';

import { cn } from '@/lib/cn';

/**
 * 对话框（Radix 无样式原语 + 本项目视觉）。
 *
 * 与既有 `Modal.tsx` 的关系：Modal 保留（已有调用点），它是"从右侧滑入的抽屉"
 * 语义；而 Dialog 是居中模态。两者都基于 Radix，因此焦点锁定、Esc 关闭、
 * 遮罩点击、aria-modal 全部到位——这是手写 Modal 做不到的。
 */
export const Dialog = DialogPrimitive.Root;
export const DialogTrigger = DialogPrimitive.Trigger;
export const DialogClose = DialogPrimitive.Close;

export function DialogContent({
  className,
  children,
  ...props
}: ComponentPropsWithoutRef<typeof DialogPrimitive.Content>) {
  return (
    <DialogPrimitive.Portal>
      <DialogPrimitive.Overlay
        className={cn(
          'fixed inset-0 z-50 bg-canvas/70 backdrop-blur-[2px]',
          'data-[state=open]:animate-in data-[state=closed]:animate-out',
          'data-[state=open]:fade-in-0 data-[state=closed]:fade-out-0',
        )}
      />
      <DialogPrimitive.Content
        className={cn(
          'fixed left-1/2 top-1/2 z-50 w-[90vw] max-w-2xl -translate-x-1/2 -translate-y-1/2',
          'rounded-xl border border-line bg-surface p-5 shadow-2xl',
          'focus-visible:outline-none',
          'data-[state=open]:animate-in data-[state=open]:fade-in-0 data-[state=open]:zoom-in-95',
          className,
        )}
        {...props}
      >
        {children}
      </DialogPrimitive.Content>
    </DialogPrimitive.Portal>
  );
}

export function DialogHeader({ className, ...props }: ComponentPropsWithoutRef<'div'>) {
  return <div className={cn('mb-4 space-y-1', className)} {...props} />;
}

export function DialogTitle({ className, ...props }: ComponentPropsWithoutRef<typeof DialogPrimitive.Title>) {
  return (
    <DialogPrimitive.Title
      className={cn('text-base font-semibold text-ink', className)}
      {...props}
    />
  );
}

export function DialogDescription({
  className,
  ...props
}: ComponentPropsWithoutRef<typeof DialogPrimitive.Description>) {
  return (
    <DialogPrimitive.Description className={cn('text-xs text-muted', className)} {...props} />
  );
}

export function DialogFooter({ className, ...props }: ComponentPropsWithoutRef<'div'>) {
  return (
    <div
      className={cn('mt-5 flex flex-wrap items-center justify-end gap-2 border-t border-line pt-4', className)}
      {...props}
    />
  );
}

/**
 * 抽屉：同一套 Radix 原语，只换定位与动画方向。
 *
 * 任务书 §4 列了 `drawer.tsx`，但那通常意味着再加一个 `vaul` 依赖。这里用
 * Dialog + 右侧定位实现，避免为一个视觉变体引入新包。
 */
export function DrawerContent({
  className,
  children,
  ...props
}: ComponentPropsWithoutRef<typeof DialogPrimitive.Content>) {
  return (
    <DialogPrimitive.Portal>
      <DialogPrimitive.Overlay className="fixed inset-0 z-50 bg-canvas/70 backdrop-blur-[2px]" />
      <DialogPrimitive.Content
        className={cn(
          'fixed inset-y-0 right-0 z-50 w-[92vw] max-w-xl border-l border-line bg-surface p-5',
          'focus-visible:outline-none',
          'data-[state=open]:animate-in data-[state=open]:slide-in-from-right data-[state=open]:fade-in-0',
          className,
        )}
        {...props}
      >
        {children}
      </DialogPrimitive.Content>
    </DialogPrimitive.Portal>
  );
}
