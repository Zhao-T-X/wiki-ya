import { useState, type FormEvent } from 'react';
import { Link, useNavigate } from 'react-router-dom';

import { DocumentCard } from '@/components/DocumentCard';
import { ActivityPanel } from '@/components/ActivityPanel';
import { ErrorNotice } from '@/components/ErrorNotice';
import { InboxIcon, ReviewIcon, SparkIcon } from '@/components/icons';
import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { EmptyState } from '@/components/ui/EmptyState';
import { Select } from '@/components/ui/Select';
import { Spinner } from '@/components/ui/Spinner';
import { Textarea } from '@/components/ui/Textarea';
import {
  analyze_document,
  create_document,
  get_extraction_run,
  get_home_overview,
  list_candidates,
  list_documents,
  start_extraction,
  WikiError,
} from '@/lib/api';
import { formatChars } from '@/lib/format';
import { useAsyncData } from '@/lib/hooks';
import { useRunEvents } from '@/lib/useRunEvents';
import { relationshipLabel } from '@/lib/status';
import { isTerminal, STAGE_LABEL, STATUS_LABEL } from '@/lib/extraction';
import { useEffect, useCallback } from 'react';
import type {
  CandidateDto,
  AnalysisReport,
  DocumentSummary,
  RunEvent,
  ExtractionRunDto,
} from '@/types/ipc';

/**
 * Home —— 整个产品唯一的入口（UX 重构）。
 *
 * 用户只需要做一件事：**把东西丢进来**。之后系统负责告诉他：
 * 处理了什么、发现了什么、哪些需要他处理。
 *
 * 这里刻意不出现 Entity / Claim / Evidence / Predicate / RRF 等内部词。
 */
export function HomePage() {
  const navigate = useNavigate();

  // PERF-06：原先这里是 5 次 IPC（app_info / list_registries / list_documents /
  // list_review_items / list_extraction_runs），每次都要开一条 SQLite 连接。
  // 现在一次 get_home_overview 取齐；下面按需派生，语义与条数上限均不变。
  const overview = useAsyncData(() => get_home_overview(), []);

  const [content, setContent] = useState('');
  const [sourceType, setSourceType] = useState('note');
  const [submitting, setSubmitting] = useState(false);
  const [formError, setFormError] = useState<WikiError | null>(null);
  const [conflictMatches, setConflictMatches] = useState<DocumentSummary[] | null>(null);
  const [created, setCreated] = useState<DocumentSummary | null>(null);

  // 捕获之后的「下一步」结果。全部来自真实调用，失败就如实说。
  const [homeRunId, setHomeRunId] = useState<string | null>(null);
  const [homeRun, setHomeRun] = useState<ExtractionRunDto | null>(null);
  const [homeCandidates, setHomeCandidates] = useState<CandidateDto[]>([]);
  const [analyzing, setAnalyzing] = useState(false);
  const [analysis, setAnalysis] = useState<AnalysisReport | null>(null);

  const aiEnabled = overview.data?.appInfo.aiEnabled ?? false;
  const sourceTypes = overview.data?.registries.sourceTypes ?? ['note'];
  const documentList = overview.data?.documents ?? [];
  const pending = overview.data?.pendingReview ?? [];
  // loading / error 统一取聚合请求的（任一段失败即整体失败，不会展示半截数据）。
  const sectionLoading = overview.loading;
  const sectionError = overview.error;

  /** 标题留空时取正文首行——降低"必须想个标题"的摩擦。 */
  function deriveTitle(): string {
    const firstLine = content
      .split('\n')
      .map((line) => line.trim())
      .find((line) => line.length > 0);
    return (firstLine ?? '未命名').slice(0, 60);
  }

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (content.trim() === '') {
      setFormError(new WikiError('INVALID_INPUT', '先粘贴或输入一些内容。'));
      return;
    }

    setSubmitting(true);
    setFormError(null);
    setConflictMatches(null);
    setCreated(null);
    setHomeCandidates([]);
    setAnalysis(null);

    try {
      const doc = await create_document({ title: deriveTitle(), content, sourceType });
      setCreated(doc);
      setContent('');
      overview.reload();
      // 捕获即抽取：AI 已启用时自动进行，用户不需要再点一次。
      // 不 await —— 让"已保存"立刻可见，抽取结果随后填充。
      if (aiEnabled) {
        void runExtraction(doc.id);
      }
    } catch (cause: unknown) {
      const error = cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause));
      if (error.code === 'CONFLICT') {
        // 契约 §1.2：CONFLICT 的 message 不含 id，需用 list_documents({ query }) 定位。
        try {
          setConflictMatches(await list_documents({ query: deriveTitle() }));
        } catch {
          setConflictMatches([]);
        }
      } else {
        setFormError(error);
      }
    } finally {
      setSubmitting(false);
    }
  }

  /**
   * 抽取指定文档的知识候选（EXTRACTION-001：异步 Run）。
   * 捕获后会**自动**调用（AI 已启用时），也可手动重跑。
   * 只创建 Run 并拿到 run_id 立即返回，真正抽取在后台跑，进度经事件实时推送。
   */
  async function runExtraction(documentId: string) {
    setFormError(null);
    setHomeCandidates([]);
    try {
      const id = await start_extraction({ id: documentId });
      setHomeRunId(id);
    } catch (cause: unknown) {
      setFormError(cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause)));
    }
  }

  // 注意：依赖必须是稳定的 `overview.reload`，不能是 `overview`。
  // useAsyncData 每次渲染都返回**新的对象字面量**，若依赖整个对象，
  // 本回调每次渲染都会重建 → 下方 [homeRunId, applyHomeRun] 的 effect
  // 每次渲染都重跑 → get_extraction_run → setHomeRun(新对象) → 重渲染，
  // 形成 IPC 级无限循环（表现为「抽取进行中页面持续卡顿」）。
  const applyHomeRun = useCallback(
    (next: ExtractionRunDto) => {
      setHomeRun(next);
      if (next.status === 'completed' && next.resultJson) {
        // PERF-04：游标分页——首页只展示首页那批候选。
        list_candidates({ id: next.id })
          .then((page) => setHomeCandidates(page.items))
          .catch(() => {});
      }
      if (isTerminal(next.status)) {
        overview.reload();
      }
    },
    [overview.reload],
  );

  const onHomeEvent = useCallback(
    (event: RunEvent) => {
      setHomeRun((prev) => {
        if (!prev) return prev;
        switch (event.kind) {
          case 'stageChanged':
            return { ...prev, stage: event.stage ?? prev.stage };
          case 'progress':
            return {
              ...prev,
              processedChunks: event.processed ?? prev.processedChunks,
              totalChunks: event.total ?? prev.totalChunks,
            };
          case 'candidateCreated':
            // 逐批增量事件：累加（M6 修正后每批发一次）。
            return {
              ...prev,
              candidatesFound: (prev.candidatesFound ?? 0) + (event.count ?? 0),
            };
          case 'completed':
            return { ...prev, status: 'completed' };
          case 'failed':
            return { ...prev, status: 'failed', errorMessage: event.error ?? prev.errorMessage };
          case 'cancelled':
            return { ...prev, status: 'cancelled' };
          default:
            return prev;
        }
      });
      if (
        event.kind === 'completed' ||
        event.kind === 'failed' ||
        event.kind === 'cancelled'
      ) {
        get_extraction_run({ id: event.runId }).then(applyHomeRun).catch(() => {});
      }
    },
    [applyHomeRun],
  );

  useRunEvents(homeRunId, onHomeEvent);

  useEffect(() => {
    if (!homeRunId) return;
    get_extraction_run({ id: homeRunId }).then(applyHomeRun).catch(() => {});
  }, [homeRunId, applyHomeRun]);

  const homeRunning = homeRun ? !isTerminal(homeRun.status) : false;

  // 依赖稳定的 `homeRunning` 布尔量，而不是每次渲染都变的 `homeRun` 对象：
  // 否则进度事件一更新 homeRun 就重建定时器，轮询永远等不到 1200ms。
  useEffect(() => {
    if (!homeRunId || !homeRunning) return;
    const timer = setInterval(() => {
      get_extraction_run({ id: homeRunId }).then(applyHomeRun).catch(() => {});
    }, 1500);
    return () => clearInterval(timer);
  }, [homeRunId, homeRunning, applyHomeRun]);

  async function runAnalysis() {
    if (!created) return;
    setAnalyzing(true);
    setFormError(null);
    try {
      const report = await analyze_document({ documentId: created.id });
      setAnalysis(report);
      overview.reload();
    } catch (cause: unknown) {
      setFormError(cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause)));
    } finally {
      setAnalyzing(false);
    }
  }

  return (
    <div className="space-y-8">
      {/* ① 主入口：把东西丢进来 */}
      <section>
        <h1 className="text-sm font-semibold text-ink">把你想记住的东西丢进来</h1>
        <p className="mt-1 text-xs text-muted">
          {aiEnabled
            ? '保存与切分在本地同步完成，随后自动抽取知识候选 —— 候选立即留痕，逐条确认后生效。'
            : '保存与切分在本地同步完成，不调用 AI，没有 API Key 也能用。'}
        </p>

        <form onSubmit={handleSubmit} className="mt-3 space-y-2">
          <Card className="p-3">
            <Textarea
              rows={6}
              value={content}
              onChange={(event) => setContent(event.target.value)}
              placeholder="粘贴一段文字、一篇文章、一段代码说明……标题会自动取自第一行。"
              className="border-none bg-transparent focus:ring-0"
            />
            <div className="mt-2 flex items-center gap-2 border-t border-line pt-2">
              <Select
                value={sourceType}
                onChange={(event) => setSourceType(event.target.value)}
                className="w-32"
              >
                {sourceTypes.map((type) => (
                  <option key={type} value={type}>
                    {type}
                  </option>
                ))}
              </Select>
              <span className="flex-1 text-meta text-muted">
                相同内容只会保留一份，不会产生副本。
              </span>
              <Button type="submit" variant="primary" loading={submitting} disabled={submitting}>
                捕获
              </Button>
            </div>
          </Card>
        </form>

        {formError ? <ErrorNotice error={formError} className="mt-3" /> : null}

        {conflictMatches ? (
          <div className="mt-3 rounded-xl border border-warn/30 bg-warn/10 p-4">
            <p className="text-sm font-medium text-warn">这份内容已经存在，未重复导入。</p>
            <p className="mt-1 text-xs leading-relaxed text-warn/90">
              系统按内容哈希做了幂等去重。下面是定位到的已有文档，可直接打开。
            </p>
            {conflictMatches.length > 0 ? (
              <div className="mt-3 space-y-2">
                {conflictMatches.map((doc) => (
                  <DocumentCard key={doc.id} doc={doc} onOpen={(id) => navigate(`/documents/${id}`)} />
                ))}
              </div>
            ) : (
              <p className="mt-2 text-xs text-warn/90">未能自动定位（可能标题不同），请在下方「最近文档」查找。</p>
            )}
          </div>
        ) : null}
      </section>

      {/* ② 捕获反馈：发生了什么 + 下一步 */}
      {created ? (
        <section>
          <Card className="border-ok/30 p-4">
            <div className="flex items-start gap-2.5">
              <SparkIcon className="mt-0.5 h-4 w-4 shrink-0 text-ok" />
              <div className="min-w-0 flex-1 text-xs leading-relaxed text-muted">
                <p className="text-sm font-medium text-ink">已捕获：{created.title}</p>
                <p className="mt-1">
                  已保存并切分为 <span className="font-medium text-ink">{created.chunkCount}</span> 个片段（
                  {formatChars(created.charCount)}），立即可被检索。
                </p>

                <div className="mt-3 flex flex-wrap gap-2">
                  {aiEnabled ? (
                    <Button
                      size="sm"
                      variant="primary"
                      loading={homeRunning}
                      disabled={homeRunning}
                      onClick={() => runExtraction(created.id)}
                    >
                      {homeCandidates.length > 0 ? '重新抽取' : '用 AI 抽取知识'}
                    </Button>
                  ) : null}
                  <Button size="sm" loading={analyzing} onClick={runAnalysis}>
                    检查知识变化
                  </Button>
                  <Button size="sm" variant="ghost" onClick={() => navigate(`/documents/${created.id}`)}>
                    查看文档
                  </Button>
                </div>

                {!aiEnabled ? (
                  <p className="mt-2 text-meta text-muted/80">
                    未配置 AI，因此不做自动抽取。你仍可在文档详情手动录入知识。
                  </p>
                ) : null}

                {aiEnabled && homeRun && homeRunning ? (
                  <p className="mt-3 flex items-center gap-2 rounded-md border border-line bg-canvas px-3 py-2">
                    <Spinner className="h-3.5 w-3.5 text-muted" />
                    {STATUS_LABEL[homeRun.status] ?? homeRun.status}
                    {homeRun.stage ? ` · ${STAGE_LABEL[homeRun.stage] ?? homeRun.stage}` : ''}
                    {homeRun.totalChunks > 0
                      ? ` （${homeRun.processedChunks}/${homeRun.totalChunks}）`
                      : ''}
                  </p>
                ) : null}

                {homeRun && homeRun.status === 'failed' ? (
                  <p className="mt-3 rounded-md border border-line bg-canvas px-3 py-2 text-warn">
                    {homeRun.errorMessage ?? '抽取失败。'}
                  </p>
                ) : null}

                {homeRun && homeCandidates.length > 0 ? (
                  <p className="mt-3 rounded-md border border-line bg-canvas px-3 py-2">
                    发现{' '}
                    <span className="font-medium text-ink">
                      {homeCandidates.filter((c) => c.status !== 'rejected').length}
                    </span>{' '}
                    条候选知识（
                    {homeCandidates.filter((c) => c.status === 'pending').length} 条待确认）。
                    <Link to={`/documents/${created.id}`} className="text-accent hover:underline">
                      逐条确认 →
                    </Link>
                  </p>
                ) : null}
                {homeRun && homeRun.status === 'failed' ? null : null}

                {analysis ? (
                  <p className="mt-2 rounded-md border border-line bg-canvas px-3 py-2">
                    {analysis.relationsWritten > 0 ? (
                      <>
                        发现 <span className="font-medium text-ink">{analysis.relationsWritten}</span> 处需要你确认的知识变化。
                        <Link to="/review" className="ml-1 text-accent hover:underline">
                          去处理 →
                        </Link>
                      </>
                    ) : (
                      '没有发现与已有知识冲突的内容。'
                    )}
                  </p>
                ) : null}
              </div>
            </div>
          </Card>
        </section>
      ) : null}

      {/* ③ 系统的待办：不需要用户主动进入某个模块 */}
      <section>
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-meta font-semibold uppercase tracking-wider text-muted">需要你处理</h2>
          {sectionLoading ? <Spinner className="h-3.5 w-3.5 text-muted" /> : null}
        </div>

        {sectionError ? <ErrorNotice error={overview.error} /> : null}

        {!sectionLoading && !sectionError && pending.length === 0 ? (
          <EmptyState
            title="没有待你处理的变化"
            description="有新信息与已有知识冲突时，会出现在这里。"
            icon={<ReviewIcon className="h-5 w-5" />}
          />
        ) : null}

        <div className="space-y-2">
          {pending.map((item) => (
            <Card key={item.relation.id} className="p-3.5">
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0 space-y-1">
                  <div className="flex items-center gap-2">
                    <Badge tone="warn">{relationshipLabel(item.relation.relationship)}</Badge>
                    <span className="truncate text-sm text-ink">{item.whatChanged}</span>
                  </div>
                  <p className="text-meta leading-relaxed text-muted">{item.impact}</p>
                </div>
                <Button size="sm" variant="secondary" onClick={() => navigate('/review')}>
                  处理
                </Button>
              </div>
            </Card>
          ))}
        </div>
      </section>

      {/* ③.5 运行记录（EXTRACTION-001）：抽取任务的实时历史 */}
      <ActivityPanel />

      {/* ④ 最近文档 */}
      <section>
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-meta font-semibold uppercase tracking-wider text-muted">最近文档</h2>
          {sectionLoading ? <Spinner className="h-3.5 w-3.5 text-muted" /> : null}
        </div>

        {sectionError ? <ErrorNotice error={overview.error} /> : null}

        {!sectionLoading && !sectionError && documentList.length === 0 ? (
          <EmptyState
            title="还没有任何文档"
            description="在上方粘贴内容并捕获，它会被切分为片段并立即可检索。"
            icon={<InboxIcon className="h-5 w-5" />}
          />
        ) : null}

        <div className="space-y-2">
          {documentList.map((doc) => (
            <DocumentCard key={doc.id} doc={doc} onOpen={(id) => navigate(`/documents/${id}`)} />
          ))}
        </div>
      </section>
    </div>
  );
}
