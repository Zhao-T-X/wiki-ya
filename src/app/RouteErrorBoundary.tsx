import { isRouteErrorResponse, useRouteError } from 'react-router-dom';

import { Card } from '@/components/ui/Card';
import { PageHeader } from '@/components/PageHeader';

/**
 * 路由级错误边界（React Router errorElement）。
 *
 * 任何页面渲染抛错时显示可恢复的错误页（含真实错误消息），而不是白屏。
 * 与「诚实优先」一致：如实展示错误，绝不静默吞掉。
 */
export function RouteErrorBoundary() {
  const error = useRouteError();

  const message = isRouteErrorResponse(error)
    ? `${error.status} ${error.statusText}`
    : error instanceof Error
      ? error.message
      : String(error);

  return (
    <div className="space-y-4">
      <PageHeader title="出错了" subtitle="页面渲染时发生未预期的错误。" />
      <Card className="space-y-3 p-5">
        <p className="text-sm font-medium text-ink">页面无法正常渲染</p>
        <pre className="overflow-x-auto whitespace-pre-wrap break-words rounded-lg border border-line bg-canvas p-3 font-mono text-[11px] leading-relaxed text-muted">
          {message}
        </pre>
        <div className="flex gap-2">
          <button
            type="button"
            onClick={() => window.location.reload()}
            className="rounded-lg border border-line bg-elevated px-3 py-1.5 text-xs font-medium text-ink transition-colors hover:text-accent"
          >
            刷新页面
          </button>
          <button
            type="button"
            onClick={() => {
              window.location.hash = '#/home';
              window.location.reload();
            }}
            className="rounded-lg border border-line bg-elevated px-3 py-1.5 text-xs font-medium text-ink transition-colors hover:text-accent"
          >
            返回首页
          </button>
        </div>
      </Card>
    </div>
  );
}
