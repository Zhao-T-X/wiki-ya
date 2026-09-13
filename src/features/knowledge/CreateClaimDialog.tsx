import { useEffect, useState, type FormEvent } from 'react';

import { ErrorNotice } from '@/components/ErrorNotice';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { Modal } from '@/components/ui/Modal';
import { Select } from '@/components/ui/Select';
import { Textarea } from '@/components/ui/Textarea';
import { create_claim, list_documents, WikiError } from '@/lib/api';
import { useAsyncData } from '@/lib/hooks';
import type { CreateClaimInput, Registries } from '@/types/ipc';

export interface CreateClaimDialogProps {
  open: boolean;
  onClose: () => void;
  /** 创建成功后回调（用于刷新实体详情）。 */
  onCreated: () => void;
  /** 受控词表；未加载时不放行提交（谓词必须 ∈ 词表）。 */
  registries: Registries | null;
  /** 预填 subject（当前实体名）。 */
  defaultSubject: string;
}

const FIELD_LABEL = 'mb-1.5 block text-[11px] font-medium text-muted';

/**
 * 手动录入 Claim —— AI 关闭时的降级录入路径（分析报告 R2）。
 * predicate / claimType / polarity / modality 全部为受控下拉，禁止自由输入。
 */
export function CreateClaimDialog({
  open,
  onClose,
  onCreated,
  registries,
  defaultSubject,
}: CreateClaimDialogProps) {
  const documents = useAsyncData(() => list_documents({ limit: 100 }), [open], open);
  const docs = documents.data;

  const [subject, setSubject] = useState(defaultSubject);
  const [predicate, setPredicate] = useState('');
  const [object, setObject] = useState('');
  const [content, setContent] = useState('');
  const [claimType, setClaimType] = useState('factual');
  const [polarity, setPolarity] = useState('positive');
  const [modality, setModality] = useState('asserted');
  const [status, setStatus] = useState('candidate');
  const [confidence, setConfidence] = useState('');
  const [documentId, setDocumentId] = useState('');
  const [quote, setQuote] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<WikiError | null>(null);

  // 每次打开重置表单。
  useEffect(() => {
    if (!open) return;
    setSubject(defaultSubject);
    setPredicate(registries?.claimPredicates[0] ?? '');
    setObject('');
    setContent('');
    setClaimType(registries?.claimTypes[0] ?? 'factual');
    setPolarity(registries?.polarities[0] ?? 'positive');
    setModality(registries?.modalities[0] ?? 'asserted');
    setStatus(registries?.claimStatuses.includes('candidate') ? 'candidate' : (registries?.claimStatuses[0] ?? 'candidate'));
    setConfidence('');
    setQuote('');
    setDocumentId('');
    setError(null);
  }, [open, defaultSubject, registries]);

  // 文档列表到达后补默认选中项。
  useEffect(() => {
    if (!open || !docs || docs.length === 0) return;
    setDocumentId((prev) => (prev === '' ? (docs[0]?.id ?? '') : prev));
  }, [open, docs]);

  const registryReady = registries !== null;
  const canSubmit = registryReady && subject.trim() !== '' && predicate !== '' && documentId !== '';

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (!canSubmit) return;

    setSubmitting(true);
    setError(null);

    const parsedConfidence = confidence.trim() === '' ? Number.NaN : Number(confidence);
    const input: CreateClaimInput = {
      subject: subject.trim(),
      predicate,
      object: object.trim() === '' ? null : object.trim(),
      content: content.trim() === '' ? null : content.trim(),
      claimType,
      polarity,
      modality,
      confidence: Number.isFinite(parsedConfidence) ? parsedConfidence : null,
      documentId,
      chunkId: null,
      quote: quote.trim() === '' ? null : quote.trim(),
      status,
    };

    try {
      await create_claim(input);
      onCreated();
      onClose();
    } catch (cause: unknown) {
      setError(cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause)));
    } finally {
      setSubmitting(false);
    }
  }

  const domainViolation = error?.code === 'DOMAIN_RULE_VIOLATION';

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="手动新建 Claim"
      description="谓词与取值均来自受控词表，越界会被领域规则拒绝（DOMAIN_RULE_VIOLATION）。"
      widthClassName="max-w-3xl"
      footer={
        <>
          <Button variant="ghost" size="sm" onClick={onClose}>
            取消
          </Button>
          <Button variant="primary" size="sm" type="submit" form="create-claim-form" loading={submitting} disabled={!canSubmit}>
            创建
          </Button>
        </>
      }
    >
      <form id="create-claim-form" className="space-y-4" onSubmit={handleSubmit}>
        {!registryReady ? (
          <ErrorNotice tone="warn">
            受控词表尚未加载（Registry 不可用），无法保证谓词合法，已禁用提交。
          </ErrorNotice>
        ) : null}

        {error ? (
          <ErrorNotice error={error}>
            {domainViolation
              ? '取值不在受控词表内，或关系两端类型不匹配，已被领域规则拒绝。'
              : undefined}
          </ErrorNotice>
        ) : null}

        {documents.error ? <ErrorNotice error={documents.error} /> : null}
        {docs && docs.length === 0 ? (
          <ErrorNotice tone="warn">
            当前没有任何文档。请先到 Inbox 捕获内容 —— Claim 必须挂在真实来源文档上。
          </ErrorNotice>
        ) : null}

        <div className="grid gap-3 sm:grid-cols-2">
          <div>
            <label className={FIELD_LABEL} htmlFor="claim-subject">
              Subject（实体名）
            </label>
            <Input
              id="claim-subject"
              value={subject}
              onChange={(event) => setSubject(event.target.value)}
              placeholder="如 Rust"
            />
          </div>

          <div>
            <label className={FIELD_LABEL} htmlFor="claim-predicate">
              Predicate（来自词表）
            </label>
            <Select
              id="claim-predicate"
              value={predicate}
              disabled={!registryReady}
              onChange={(event) => setPredicate(event.target.value)}
            >
              {(registries?.claimPredicates ?? []).map((item) => (
                <option key={item} value={item}>
                  {item}
                </option>
              ))}
            </Select>
          </div>

          <div>
            <label className={FIELD_LABEL} htmlFor="claim-object">
              Object（可选，实体名或字面量）
            </label>
            <Input
              id="claim-object"
              value={object}
              onChange={(event) => setObject(event.target.value)}
              placeholder="如 async fn in trait"
            />
          </div>

          <div>
            <label className={FIELD_LABEL} htmlFor="claim-document">
              Source Document（必填）
            </label>
            <Select
              id="claim-document"
              value={documentId}
              disabled={documents.loading || (docs?.length ?? 0) === 0}
              onChange={(event) => setDocumentId(event.target.value)}
            >
              {(docs ?? []).map((doc) => (
                <option key={doc.id} value={doc.id}>
                  {doc.title}
                </option>
              ))}
            </Select>
          </div>

          <div>
            <label className={FIELD_LABEL} htmlFor="claim-type">
              Claim Type
            </label>
            <Select
              id="claim-type"
              value={claimType}
              disabled={!registryReady}
              onChange={(event) => setClaimType(event.target.value)}
            >
              {(registries?.claimTypes ?? []).map((item) => (
                <option key={item} value={item}>
                  {item}
                </option>
              ))}
            </Select>
          </div>

          <div>
            <label className={FIELD_LABEL} htmlFor="claim-polarity">
              Polarity
            </label>
            <Select
              id="claim-polarity"
              value={polarity}
              disabled={!registryReady}
              onChange={(event) => setPolarity(event.target.value)}
            >
              {(registries?.polarities ?? []).map((item) => (
                <option key={item} value={item}>
                  {item}
                </option>
              ))}
            </Select>
          </div>

          <div>
            <label className={FIELD_LABEL} htmlFor="claim-modality">
              Modality
            </label>
            <Select
              id="claim-modality"
              value={modality}
              disabled={!registryReady}
              onChange={(event) => setModality(event.target.value)}
            >
              {(registries?.modalities ?? []).map((item) => (
                <option key={item} value={item}>
                  {item}
                </option>
              ))}
            </Select>
          </div>

          <div>
            <label className={FIELD_LABEL} htmlFor="claim-status">
              Status（默认 candidate）
            </label>
            <Select
              id="claim-status"
              value={status}
              disabled={!registryReady}
              onChange={(event) => setStatus(event.target.value)}
            >
              {(registries?.claimStatuses ?? []).map((item) => (
                <option key={item} value={item}>
                  {item}
                </option>
              ))}
            </Select>
          </div>

          <div>
            <label className={FIELD_LABEL} htmlFor="claim-confidence">
              Confidence（0–1，可留空）
            </label>
            <Input
              id="claim-confidence"
              type="number"
              min={0}
              max={1}
              step={0.05}
              value={confidence}
              onChange={(event) => setConfidence(event.target.value)}
              placeholder="留空表示未知"
            />
          </div>
        </div>

        <div>
          <label className={FIELD_LABEL} htmlFor="claim-content">
            Content（可选，自由文本陈述）
          </label>
          <Textarea
            id="claim-content"
            rows={2}
            value={content}
            onChange={(event) => setContent(event.target.value)}
            placeholder="如 Rust supports async fn in trait"
          />
        </div>

        <div>
          <label className={FIELD_LABEL} htmlFor="claim-quote">
            Quote（可选，证据引文）
          </label>
          <Textarea
            id="claim-quote"
            rows={2}
            value={quote}
            onChange={(event) => setQuote(event.target.value)}
            placeholder="从来源文档中摘录的原文片段"
          />
        </div>
      </form>
    </Modal>
  );
}
