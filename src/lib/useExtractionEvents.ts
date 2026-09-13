import { useEffect, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';

import type { ExtractionEvent } from '@/types/ipc';

/**
 * 订阅指定 runId 的抽取运行事件流（EXTRACTION-001）。
 *
 * 后端经 Tauri 全局频道 `extraction-events` 推送 `ExtractionEvent`；
 * 本 hook 按 runId 过滤。浏览器预览（非 Tauri）下 `listen` 不可用，
 * 静默降级为无事件（UI 走 `get_extraction_run` 的完整快照，不会崩）。
 */
export function useExtractionEvents(
  runId: string | null,
  onEvent: (event: ExtractionEvent) => void,
) {
  const handlerRef = useRef(onEvent);
  handlerRef.current = onEvent;

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;

    listen<ExtractionEvent>('extraction-events', (event) => {
      // runId 为 null 时订阅全部事件（Activity 全局刷新）；否则按 run 过滤。
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
