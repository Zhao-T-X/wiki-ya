import { useCallback, useEffect, useRef, useState } from 'react';

import { WikiError } from '@/lib/api';

export interface AsyncState<T> {
  data: T | null;
  loading: boolean;
  error: WikiError | null;
  /** 重新执行 loader（用于提交后的刷新）。 */
  reload: () => void;
  /** 本地乐观更新（避免整表重取）。 */
  setData: (updater: T | ((prev: T | null) => T | null)) => void;
}

function normalizeError(error: unknown): WikiError {
  if (error instanceof WikiError) return error;
  return new WikiError('INTERNAL_ERROR', error instanceof Error ? error.message : String(error));
}

/**
 * 轻量异步数据读取 hook。仅用于「读取并展示」，不承担缓存职责
 * （技术设计文档 §58：知识库数据不进 Zustand）。
 *
 * @param loader 读取函数；内部用 ref 持有，避免调用方每次渲染新建函数导致的重复请求。
 * @param deps   依赖数组；变化时重新执行。
 * @param enabled 为 false 时不执行（用于条件加载）。
 */
export function useAsyncData<T>(
  loader: () => Promise<T>,
  deps: readonly unknown[],
  enabled = true,
): AsyncState<T> {
  const [data, setData] = useState<T | null>(null);
  const [loading, setLoading] = useState<boolean>(enabled);
  const [error, setError] = useState<WikiError | null>(null);
  const [nonce, setNonce] = useState(0);

  const loaderRef = useRef(loader);
  loaderRef.current = loader;

  useEffect(() => {
    if (!enabled) {
      setData(null);
      setError(null);
      setLoading(false);
      return;
    }

    let cancelled = false;
    setLoading(true);
    setError(null);
    // 依赖变化时先清空旧数据：否则切换实体/参数时会继续展示上一次的结果，
    // 甚至在新请求 NOT_FOUND 时残留与当前 URL 不符的详情。
    setData(null);

    loaderRef
      .current()
      .then((result) => {
        if (!cancelled) setData(result);
      })
      .catch((cause: unknown) => {
        if (!cancelled) setError(normalizeError(cause));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
    // deps 由调用方保证长度稳定。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, enabled, nonce]);

  const reload = useCallback(() => {
    setNonce((value) => value + 1);
  }, []);

  const update = useCallback((updater: T | ((prev: T | null) => T | null)) => {
    setData((prev) => (typeof updater === 'function' ? (updater as (p: T | null) => T | null)(prev) : updater));
  }, []);

  return { data, loading, error, reload, setData: update };
}

/** 输入防抖：用于「实时筛选」场景，避免每次按键都打 IPC。 */
export function useDebouncedValue<T>(value: T, delayMs = 250): T {
  const [debounced, setDebounced] = useState(value);

  useEffect(() => {
    const timer = window.setTimeout(() => setDebounced(value), delayMs);
    return () => window.clearTimeout(timer);
  }, [value, delayMs]);

  return debounced;
}
