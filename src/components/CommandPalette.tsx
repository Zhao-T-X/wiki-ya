import { useEffect, useMemo, useRef, useState } from 'react';
import type { KeyboardEvent as ReactKeyboardEvent } from 'react';
import { createPortal } from 'react-dom';
import { useNavigate } from 'react-router-dom';

import { SearchIcon } from '@/components/icons';
import { Badge } from '@/components/ui/Badge';
import { Spinner } from '@/components/ui/Spinner';
import { search, WikiError } from '@/lib/api';
import { cn } from '@/lib/cn';
import { formatScore } from '@/lib/format';
import { hitTarget } from '@/lib/links';
import { searchKindLabel } from '@/lib/status';
import { useUiStore } from '@/stores/ui';
import type { SearchHit, SearchResponse } from '@/types/ipc';

const DEBOUNCE_MS = 180;

export function CommandPalette() {
  const open = useUiStore((state) => state.commandPaletteOpen);
  const close = useUiStore((state) => state.closeCommandPalette);
  const navigate = useNavigate();

  const [query, setQuery] = useState('');
  const [response, setResponse] = useState<SearchResponse | null>(null);
  const [error, setError] = useState<WikiError | null>(null);
  const [loading, setLoading] = useState(false);
  const [selected, setSelected] = useState(0);

  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!open) return;
    setQuery('');
    setResponse(null);
    setError(null);
    setSelected(0);
    const timer = window.setTimeout(() => inputRef.current?.focus(), 0);
    return () => window.clearTimeout(timer);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const trimmed = query.trim();
    if (trimmed.length === 0) {
      setResponse(null);
      setError(null);
      setLoading(false);
      return;
    }

    let cancelled = false;
    setLoading(true);
    const timer = window.setTimeout(() => {
      search({ query: trimmed, limit: 8 })
        .then((result) => {
          if (cancelled) return;
          setResponse(result);
          setError(null);
        })
        .catch((cause: unknown) => {
          if (cancelled) return;
          setError(cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause)));
        })
        .finally(() => {
          if (!cancelled) setLoading(false);
        });
    }, DEBOUNCE_MS);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [open, query]);

  const hits = useMemo(() => response?.hits ?? [], [response]);

  if (!open) return null;

  const activate = (hit: SearchHit) => {
    const target = hitTarget(hit);
    if (!target) return;
    close();
    navigate(target);
  };

  const onKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Escape') {
      event.preventDefault();
      close();
      return;
    }
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      setSelected((value) => Math.min(value + 1, Math.max(hits.length - 1, 0)));
      return;
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault();
      setSelected((value) => Math.max(value - 1, 0));
      return;
    }
    if (event.key === 'Enter') {
      event.preventDefault();
      const hit = hits[selected];
      if (hit) activate(hit);
    }
  };

  return createPortal(
    <div
      className="fixed inset-0 z-50 overflow-y-auto bg-black/60 p-6 backdrop-blur-sm"
      onMouseDown={close}
      role="presentation"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="全局搜索"
        className="mx-auto mt-16 w-full max-w-xl overflow-hidden rounded-xl border border-line bg-surface shadow-2xl shadow-black/50"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="flex items-center gap-2.5 border-b border-line px-4 py-3">
          <SearchIcon className="h-4 w-4 shrink-0 text-muted" />
          <input
            ref={inputRef}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={onKeyDown}
            placeholder="搜索知识库…"
            className="h-6 w-full bg-transparent text-sm text-ink outline-none placeholder:text-muted/70"
          />
          {loading ? <Spinner className="h-3.5 w-3.5 text-muted" /> : null}
        </div>

        <div className="max-h-[380px] overflow-y-auto">
          {error ? (
            <p className="px-4 py-6 text-center text-xs text-danger">{error.message}</p>
          ) : null}

          {!error && query.trim().length === 0 ? (
            <p className="px-4 py-6 text-center text-xs text-muted">输入关键字开始检索（仅本地 FTS）</p>
          ) : null}

          {!error && query.trim().length > 0 && !loading && hits.length === 0 ? (
            <p className="px-4 py-6 text-center text-xs text-muted">没有匹配结果</p>
          ) : null}

          {hits.length > 0 ? (
            <ul className="p-1.5">
              {hits.map((hit, index) => {
                const target = hitTarget(hit);
                return (
                  <li key={`${hit.kind}-${hit.id}`}>
                    <button
                      type="button"
                      disabled={!target}
                      onMouseEnter={() => setSelected(index)}
                      onClick={() => activate(hit)}
                      className={cn(
                        'flex w-full items-start gap-3 rounded-lg px-3 py-2.5 text-left transition-colors disabled:opacity-50',
                        index === selected ? 'bg-elevated' : 'hover:bg-elevated/60',
                      )}
                    >
                      <Badge tone="accent" className="mt-0.5 shrink-0">
                        {searchKindLabel(hit.kind)}
                      </Badge>
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-sm text-ink">{hit.title}</span>
                        <span className="mt-0.5 block truncate text-[11px] text-muted">{hit.snippet}</span>
                      </span>
                      <span className="shrink-0 font-mono text-[10px] text-muted">{formatScore(hit.score)}</span>
                    </button>
                  </li>
                );
              })}
            </ul>
          ) : null}
        </div>

        {response ? (
          <div className="flex items-center justify-between border-t border-line px-4 py-2 text-[10px] text-muted">
            <span>
              {response.total} 条结果 · {response.method} · {response.tookMs} ms
            </span>
            <span>↑↓ 选择 · Enter 打开 · Esc 关闭</span>
          </div>
        ) : null}
      </div>
    </div>,
    document.body,
  );
}
