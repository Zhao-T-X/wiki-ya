import { useState, type FormEvent } from 'react';
import { useNavigate } from 'react-router-dom';

import { DocumentCard } from '@/components/DocumentCard';
import { ErrorNotice } from '@/components/ErrorNotice';
import { PageHeader } from '@/components/PageHeader';
import { InboxIcon, SparkIcon } from '@/components/icons';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { EmptyState } from '@/components/ui/EmptyState';
import { Input } from '@/components/ui/Input';
import { Select } from '@/components/ui/Select';
import { Spinner } from '@/components/ui/Spinner';
import { Textarea } from '@/components/ui/Textarea';
import { create_document, list_documents, list_registries, WikiError } from '@/lib/api';
import { formatChars } from '@/lib/format';
import { useAsyncData } from '@/lib/hooks';
import type { DocumentSummary } from '@/types/ipc';

export function InboxPage() {
  const navigate = useNavigate();

  const registries = useAsyncData(() => list_registries(), []);
  const documents = useAsyncData(() => list_documents({ limit: 30 }), []);

  const [title, setTitle] = useState('');
  const [content, setContent] = useState('');
  const [sourceType, setSourceType] = useState('note');
  const [submitting, setSubmitting] = useState(false);
  const [formError, setFormError] = useState<WikiError | null>(null);
  const [conflictMatches, setConflictMatches] = useState<DocumentSummary[] | null>(null);
  const [created, setCreated] = useState<DocumentSummary | null>(null);

  const sourceTypes = registries.data?.sourceTypes ?? ['note'];
  const documentList = documents.data ?? [];

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();

    if (title.trim() === '' || content.trim() === '') {
      setFormError(new WikiError('INVALID_INPUT', '标题与正文都不能为空。'));
      return;
    }

    setSubmitting(true);
    setFormError(null);
    setConflictMatches(null);
    setCreated(null);

    try {
      const doc = await create_document({ title: title.trim(), content, sourceType });
      setCreated(doc);
      setTitle('');
      setContent('');
      documents.reload();
    } catch (cause: unknown) {
      const error = cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause));

      if (error.code === 'CONFLICT') {
        // 契约 §1.2：CONFLICT 的 message 不含 id，需用 list_documents({ query }) 定位已存在文档。
        try {
          const matches = await list_documents({ query: title.trim() });
          setConflictMatches(matches);
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

  return (
    <div>
      <PageHeader
        title="Inbox"
        subtitle="所有知识进入系统的入口。捕获在本地同步完成（写文档 + 切分），不调用 AI，无 Key 也能用。"
      />

      <form onSubmit={handleSubmit} className="space-y-3">
        <Card className="p-4">
          <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_180px]">
            <div>
              <label className="mb-1.5 block text-[11px] font-medium text-muted" htmlFor="inbox-title">
                标题
              </label>
              <Input
                id="inbox-title"
                value={title}
                onChange={(event) => setTitle(event.target.value)}
                placeholder="给这段内容一个可检索的标题"
              />
            </div>
            <div>
              <label className="mb-1.5 block text-[11px] font-medium text-muted" htmlFor="inbox-source">
                来源类型
              </label>
              <Select
                id="inbox-source"
                value={sourceType}
                onChange={(event) => setSourceType(event.target.value)}
              >
                {sourceTypes.map((type) => (
                  <option key={type} value={type}>
                    {type}
                  </option>
                ))}
              </Select>
            </div>
          </div>

          <div className="mt-3">
            <label className="mb-1.5 block text-[11px] font-medium text-muted" htmlFor="inbox-content">
              正文（Markdown / 纯文本）
            </label>
            <Textarea
              id="inbox-content"
              rows={8}
              value={content}
              onChange={(event) => setContent(event.target.value)}
              placeholder="粘贴或输入内容，例如：Rust 现在已经支持 async fn in trait。"
            />
          </div>

          <div className="mt-3 flex items-center justify-between gap-3">
            <p className="text-[11px] text-muted">
              相同内容（SHA-256）只会保留一份，重复提交会被拒绝而不会产生副本。
            </p>
            <Button type="submit" variant="primary" loading={submitting} disabled={submitting}>
              捕获到 Inbox
            </Button>
          </div>
        </Card>
      </form>

      {formError ? <ErrorNotice error={formError} className="mt-3" /> : null}

      {conflictMatches ? (
        <div className="mt-3 rounded-xl border border-warn/30 bg-warn/10 p-4">
          <p className="text-sm font-medium text-warn">这份内容已经存在，未重复导入。</p>
          <p className="mt-1 text-xs leading-relaxed text-warn/90">
            系统按内容哈希做幂等去重。下面是系统为你定位到的已有文档，可直接打开查看，无需再次捕获。
          </p>
          {conflictMatches.length > 0 ? (
            <div className="mt-3 space-y-2">
              {conflictMatches.map((doc) => (
                <DocumentCard key={doc.id} doc={doc} onOpen={(id) => navigate(`/documents/${id}`)} />
              ))}
            </div>
          ) : (
            <p className="mt-2 text-xs text-warn/90">
              未能自动定位到相同内容（可能标题不同）。请在下方「最近文档」中查找。
            </p>
          )}
        </div>
      ) : null}

      {created ? (
        <Card className="mt-3 border-ok/30 p-4">
          <div className="flex items-start gap-2.5">
            <SparkIcon className="mt-0.5 h-4 w-4 shrink-0 text-ok" />
            <div className="min-w-0 text-xs leading-relaxed text-muted">
              <p className="text-sm font-medium text-ink">已捕获：{created.title}</p>
              <p className="mt-1">
                本地已完成确定性处理：生成 <span className="font-medium text-ink">{created.chunkCount}</span> 个片段（
                {formatChars(created.charCount)}）。系统会在后台继续处理。
              </p>
              <p className="mt-1">
                注意：AI 抽取尚未启用（Phase 6），因此当前不会自动生成 Claim。你可以在 Knowledge 中手动录入，
                或在文档详情运行确定性分析来发现潜在的知识变更。
              </p>
              <div className="mt-3 flex gap-2">
                <Button size="sm" onClick={() => navigate(`/documents/${created.id}`)}>
                  查看文档与片段
                </Button>
              </div>
            </div>
          </div>
        </Card>
      ) : null}

      <section className="mt-8">
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-[11px] font-semibold uppercase tracking-wider text-muted">最近文档</h2>
          {documents.loading ? <Spinner className="h-3.5 w-3.5 text-muted" /> : null}
        </div>

        {documents.error ? <ErrorNotice error={documents.error} /> : null}

        {!documents.loading && !documents.error && documentList.length === 0 ? (
          <EmptyState
            title="还没有任何文档"
            description="在上方输入内容并捕获，它会被切分为片段并立即可检索。"
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
