import { useEffect, useRef } from 'react';

import { listen } from '@tauri-apps/api/event';

import type { RunEvent } from '@/types/ipc';

/**
 * 订阅统一 Run 事件（M1，频道 `run-events`）。
 *
 * - `runId` 传入时只接收该 Run 的事件（面板 / 详情页用）；
 * - 传 `null` 时接收全部 Run 的事件（Activity 全局刷新用）。
 * - 非桌面环境（浏览器预览）静默降级为不订阅。
 */
export function useRunEvents(runId: string | null, onEvent: (event: RunEvent) => void) {
  const handlerRef = useRef(onEvent);
  handlerRef.current = onEvent;

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;

    listen<RunEvent>('run-events', (event) => {
      if (runId && event.payload?.runId !== runId) return;
      handlerRef.current(event.payload);
    })
      .then((stop) => {
        if (cancelled) stop();
        else unlisten = stop;
      })
      .catch(() => {
        // 浏览器预览：无 Tauri IPC，静默降级。
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [runId]);
}
