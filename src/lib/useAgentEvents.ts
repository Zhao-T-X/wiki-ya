import { useEffect, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';

import type { AgentEvent } from '@/types/ipc';

/**
 * 订阅指定 runId 的 Agent 事件流（TDD §53/§54）。
 *
 * 后端经 Tauri 全局 channel `agent-events` 推送 `AppEvent`；
 * 本 hook 按 runId 过滤。浏览器预览（非 Tauri）下 `listen` 不可用，
 * 静默降级为无事件（UI 走命令返回的完整结果，不会崩）。
 */
export function useAgentEvents(runId: string | null, onEvent: (event: AgentEvent) => void) {
  const handlerRef = useRef(onEvent);
  handlerRef.current = onEvent;

  useEffect(() => {
    if (!runId) return;

    let cancelled = false;
    let unlisten: (() => void) | null = null;

    listen<AgentEvent>('agent-events', (event) => {
      if (event.payload?.run_id !== runId) return;
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
