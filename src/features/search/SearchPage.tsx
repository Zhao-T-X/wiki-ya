import { useState } from 'react';
import { useNavigate } from 'react-router-dom';

import { SearchAnswerPanel } from '@/features/search/SearchAnswerPanel';
import { SearchBar, type SearchMode } from '@/features/search/SearchBar';
import { SearchFilters } from '@/features/search/SearchFilters';
import { SearchResultList } from '@/features/search/SearchResultList';
import { ErrorNotice } from '@/components/ErrorNotice';
import { PageHeader } from '@/components/PageHeader';
import { app_info, ask, search, WikiError } from '@/lib/api';
import { newRunId } from '@/lib/format';
import { useAsyncData } from '@/lib/hooks';
import { useRunEvents } from '@/lib/useRunEvents';
import type { AskResponse, SearchKind, SearchResponse } from '@/types/ipc';

/** 默认返回条数。任务书 §28：有限加载，不得无限制拉取。 */
const DEFAULT_LIMIT = 20;

const DEFAULT_KINDS: SearchKind[] = ['claim', 'entity', 'document'];

/**
 * Search = 找到可信的知识。
 *
 * 页面只做**编排**：输入、筛选、结果、答案四块各自是独立组件
 * （任务书 §34 的拆分标准：页面状态 / 列表状态 / 详情状态 / 内容渲染 / 基础视觉）。
 *
 * 信息层级（任务书 §5.2）：
 * - Level 1：答案正文、来源、知识标题
 * - Level 2：类型标签、段号、支持度
 * - Level 3：score / method / matchedIn / token —— 默认折叠，不进主界面
 *
 * 性能（任务书 §28）：输入不触发 IPC；提交才检索；limit 上限 20；
 * AI 回答不自动执行，必须显式切换到「问答」模式。
 */
export function SearchPage() {
  const navigate = useNavigate();
  const appInfo = useAsyncData(() => app_info(), []);
  const aiEnabled = appInfo.data?.aiEnabled ?? false;

  const [mode, setMode] = useState<SearchMode>('search');
  const [query, setQuery] = useState('');
  const [kinds, setKinds] = useState<SearchKind[]>([...DEFAULT_KINDS]);

  const [response, setResponse] = useState<SearchResponse | null>(null);
  const [searching, setSearching] = useState(false);
  const [searched, setSearched] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);

  const [answer, setAnswer] = useState<AskResponse | null>(null);
  const [answering, setAnswering] = useState(false);
  const [answerError, setAnswerError] = useState<string | null>(null);
  const [runId, setRunId] = useState<string | null>(null);
  const [streaming, setStreaming] = useState('');

  useRunEvents(runId, (event) => {
    if (event.kind === 'tokenDelta') setStreaming((prev) => prev + event.delta);
  });

  function switchMode(next: SearchMode) {
    setMode(next);
    // 两种模式的数据互不相干（ask 自己检索），留着上一份只会让人困惑。
    if (next === 'ask') setAnswerError(null);
    else {
      setAnswer(null);
      setStreaming('');
    }
  }

  async function runSearch(raw: string) {
    setSearching(true);
    setSearchError(null);
    setSearched(true);
    setResponse(null);
    try {
      setResponse(
        await search({
          query: raw,
          limit: DEFAULT_LIMIT,
          semantic: false,
          kinds,
        }),
      );
    } catch (cause: unknown) {
      setSearchError(toMessage(cause));
    } finally {
      setSearching(false);
    }
  }

  async function runAnswer(question: string) {
    if (answering) return;
    const id = newRunId();
    setRunId(id);
    setStreaming('');
    setAnswering(true);
    setAnswerError(null);
    try {
      setAnswer(await ask({ question, role: 'auto', runId: id }));
    } catch (cause: unknown) {
      setAnswerError(toMessage(cause));
      setAnswer(null);
    } finally {
      setStreaming('');
      setRunId(null);
      setAnswering(false);
    }
  }

  function submit() {
    const trimmed = query.trim();
    if (trimmed === '') return;
    if (mode === 'ask') void runAnswer(trimmed);
    else void runSearch(trimmed);
  }

  return (
    <div className="mx-auto w-full max-w-search">
      <PageHeader
        title="Search"
        subtitle="搜知识库，或让 AI 基于你的知识库作答。答案里的 [n] 可以点开对应来源。"
      />

      {appInfo.error ? <ErrorNotice error={appInfo.error} /> : null}

      <div className="space-y-4">
        <SearchBar
          query={query}
          onQueryChange={setQuery}
          mode={mode}
          onModeChange={switchMode}
          onSubmit={submit}
          loading={mode === 'ask' ? answering : searching}
          aiEnabled={aiEnabled}
        />

        {mode === 'search' ? (
          <SearchFilters
            kinds={kinds}
            onKindsChange={setKinds}
            resultCount={searched && !searching ? (response?.total ?? 0) : undefined}
          />
        ) : null}

        {mode === 'search' ? (
          <SearchResultList
            response={response}
            loading={searching}
            submitted={searched}
            error={searchError}
            onOpen={(path) => navigate(path)}
          />
        ) : (
          <SearchAnswerPanel
            answer={answer}
            streaming={streaming}
            answering={answering}
            error={answerError}
            onCitation={() => undefined}
          />
        )}
      </div>
    </div>
  );
}

function toMessage(cause: unknown): string {
  if (cause instanceof WikiError) return cause.message;
  return cause instanceof Error ? cause.message : String(cause);
}
