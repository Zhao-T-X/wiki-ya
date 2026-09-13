import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';

import { ErrorNotice } from '@/components/ErrorNotice';
import { PageHeader } from '@/components/PageHeader';
import { CopyIcon } from '@/components/icons';
import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { Input } from '@/components/ui/Input';
import { Spinner } from '@/components/ui/Spinner';
import { Stat } from '@/components/ui/Stat';
import {
  app_info,
  get_settings,
  knowledge_health,
  list_registries,
  update_settings,
  WikiError,
} from '@/lib/api';
import { cn } from '@/lib/cn';
import { useAsyncData } from '@/lib/hooks';
import type { Tone } from '@/lib/status';
import { useUiStore } from '@/stores/ui';
import type { HealthReport, Registries, UpdateAiSettings } from '@/types/ipc';

interface HealthMetric {
  key: keyof HealthReport;
  label: string;
  tone: Tone;
  /** 对应 Review 的关系类型筛选；null 表示无对应关系类型。 */
  filter: string | null;
  hint?: string;
}

/**
 * Knowledge Health 指标 → Review 筛选的映射。
 * 只有与「claim 间演化关系」直接相关的指标才有对应关系类型，其余跳转到 Review 全量队列。
 */
const HEALTH_METRICS: HealthMetric[] = [
  { key: 'potentialDuplicates', label: '潜在重复', tone: 'warn', filter: 'duplicate' },
  { key: 'unresolvedConflicts', label: '未解决冲突', tone: 'danger', filter: 'contradicts' },
  { key: 'claimsWithoutEvidence', label: '无证据 Claim', tone: 'warn', filter: null, hint: '需在 Knowledge 逐条补证据' },
  { key: 'unresolvedEntities', label: '未解决实体', tone: 'warn', filter: null, hint: '需在 Knowledge 合并/确认' },
  { key: 'supersededClaims', label: '已取代 Claim', tone: 'neutral', filter: 'supersedes' },
];

type RegistryTabId =
  | 'entityTypes'
  | 'claimPredicates'
  | 'relationPredicates'
  | 'claimTypes'
  | 'polarityModality'
  | 'statuses'
  | 'other';

const REGISTRY_TABS: { id: RegistryTabId; label: string }[] = [
  { id: 'entityTypes', label: 'Entity Types' },
  { id: 'claimPredicates', label: 'Claim Predicates' },
  { id: 'relationPredicates', label: 'Relation Predicates' },
  { id: 'claimTypes', label: 'Claim Types' },
  { id: 'polarityModality', label: 'Polarity / Modality' },
  { id: 'statuses', label: 'Statuses' },
  { id: 'other', label: '其他' },
];

function ChipList({ items }: { items: string[] }) {
  if (items.length === 0) return <p className="text-xs text-muted">（空）</p>;
  return (
    <div className="flex flex-wrap gap-1.5">
      {items.map((item) => (
        <Badge key={item}>{item}</Badge>
      ))}
    </div>
  );
}

function RegistrySection({ title, items }: { title: string; items: string[] }) {
  return (
    <div className="mb-5 last:mb-0">
      <p className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-muted">
        {title} <span className="font-normal text-muted/60">（{items.length}）</span>
      </p>
      <ChipList items={items} />
    </div>
  );
}

function RegistryContent({ tab, registries }: { tab: RegistryTabId; registries: Registries }) {
  switch (tab) {
    case 'entityTypes':
      return (
        <ul className="grid gap-1.5 sm:grid-cols-2">
          {registries.entityTypes.map((type) => (
            <li key={type.value} className="rounded-lg border border-line bg-canvas px-3 py-2">
              <p className="font-mono text-xs text-ink">{type.value}</p>
              <p className="mt-0.5 text-[11px] leading-relaxed text-muted">{type.description}</p>
            </li>
          ))}
        </ul>
      );

    case 'claimPredicates':
      return <RegistrySection title="Claim Predicate" items={registries.claimPredicates} />;

    case 'relationPredicates':
      return (
        <ul className="space-y-1.5">
          {registries.relationPredicates.map((item) => (
            <li key={item.predicate} className="rounded-lg border border-line bg-canvas px-3 py-2">
              <div className="flex flex-wrap items-center gap-2">
                <span className="font-mono text-xs text-ink">{item.predicate}</span>
                <span className="text-[10px] text-muted">↔ {item.inverseLabel}</span>
                {item.symmetric ? <Badge tone="accent">对称</Badge> : null}
                {item.transitive ? <Badge tone="ok">传递</Badge> : null}
              </div>
              <p className="mt-1 font-mono text-[10px] text-muted/80">
                {item.sourceTypes.join(', ')} → {item.targetTypes.join(', ')}
              </p>
            </li>
          ))}
        </ul>
      );

    case 'claimTypes':
      return <RegistrySection title="Claim Type" items={registries.claimTypes} />;

    case 'polarityModality':
      return (
        <>
          <RegistrySection title="Polarity" items={registries.polarities} />
          <RegistrySection title="Modality" items={registries.modalities} />
        </>
      );

    case 'statuses':
      return (
        <>
          <RegistrySection title="Claim Status" items={registries.claimStatuses} />
          <RegistrySection title="Entity Status" items={registries.entityStatuses} />
          <RegistrySection title="Relation Status" items={registries.relationStatuses} />
          <RegistrySection title="Claim Relation Status" items={registries.claimRelationStatuses} />
          <RegistrySection title="Idea Status" items={registries.ideaStatuses} />
          <RegistrySection title="Question Status" items={registries.questionStatuses} />
          <RegistrySection title="Event Status" items={registries.eventStatuses} />
          <RegistrySection title="Research Task Status" items={registries.researchTaskStatuses} />
        </>
      );

    case 'other':
      return (
        <>
          <RegistrySection title="Claim Relation Type" items={registries.claimRelationTypes} />
          <RegistrySection title="Question Type" items={registries.questionTypes} />
          <RegistrySection title="Event Type" items={registries.eventTypes} />
          <RegistrySection title="Event Time Precision" items={registries.eventTimePrecisions} />
          <RegistrySection title="Source Type" items={registries.sourceTypes} />
          <RegistrySection title="Normalization Outcome" items={registries.normalizationOutcomes} />
          <RegistrySection title="Entity Resolution Step" items={registries.entityResolutionSteps} />
          <RegistrySection title="Load Strategy" items={registries.loadStrategies} />
          <RegistrySection title="Agent Role" items={registries.agentRoles} />
        </>
      );

    default:
      return null;
  }
}

export function SettingsPage() {
  const navigate = useNavigate();
  const setReviewFilter = useUiStore((state) => state.setReviewFilter);
  const theme = useUiStore((state) => state.theme);
  const setTheme = useUiStore((state) => state.setTheme);

  const appInfo = useAsyncData(() => app_info(), []);
  const health = useAsyncData(() => knowledge_health(), []);
  const registries = useAsyncData(() => list_registries(), []);

  const aiSettings = useAsyncData(() => get_settings(), []);
  const [aiForm, setAiForm] = useState({ apiKey: '', baseUrl: '', model: '', embeddingModel: '', tokenBudget: '' });
  const [aiSaving, setAiSaving] = useState(false);
  const [aiSaved, setAiSaved] = useState(false);
  const [aiError, setAiError] = useState<WikiError | null>(null);

  // 设置加载完成后回填表单（API Key 不回显明文，留空表示「不修改」）。
  useEffect(() => {
    if (aiSettings.data) {
      setAiForm({
        apiKey: '',
        baseUrl: aiSettings.data.baseUrl,
        model: aiSettings.data.model,
        embeddingModel: aiSettings.data.embeddingModel,
        tokenBudget: String(aiSettings.data.tokenBudget),
      });
    }
  }, [aiSettings.data]);

  async function saveAiSettings() {
    setAiSaving(true);
    setAiSaved(false);
    setAiError(null);
    try {
      const payload: UpdateAiSettings = {
        apiKey: aiForm.apiKey.trim() || undefined,
        baseUrl: aiForm.baseUrl,
        model: aiForm.model,
        embeddingModel: aiForm.embeddingModel,
        tokenBudget: aiForm.tokenBudget.trim() ? Number(aiForm.tokenBudget) : undefined,
      };
      await update_settings(payload);
      setAiForm((form) => ({ ...form, apiKey: '' })); // 清空明文输入
      aiSettings.reload();
      appInfo.reload(); // 即时刷新顶部的「AI 运行时」状态
      setAiSaved(true);
    } catch (cause: unknown) {
      setAiError(
        cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause)),
      );
    } finally {
      setAiSaving(false);
    }
  }

  async function clearAiKey() {
    setAiSaving(true);
    setAiError(null);
    try {
      await update_settings({ apiKey: '' }); // 空字符串 = 显式清除
      aiSettings.reload();
      appInfo.reload();
      setAiSaved(true);
    } catch (cause: unknown) {
      setAiError(
        cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause)),
      );
    } finally {
      setAiSaving(false);
    }
  }

  const [copied, setCopied] = useState(false);
  const [tab, setTab] = useState<RegistryTabId>('entityTypes');

  async function copyDbPath() {
    const path = appInfo.data?.dbPath;
    if (!path) return;
    try {
      await navigator.clipboard.writeText(path);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      setCopied(false);
    }
  }

  function openReview(filter: string | null) {
    setReviewFilter(filter);
    navigate('/review');
  }

  const info = appInfo.data;
  const report = health.data;

  return (
    <div className="space-y-8">
      <PageHeader title="Settings" subtitle="应用信息、Knowledge Health 与受控词表浏览器。" />

      <section>
        <h2 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-muted">应用信息</h2>
        {appInfo.error ? <ErrorNotice error={appInfo.error} /> : null}
        {appInfo.loading && !info ? (
          <div className="flex items-center gap-2 text-xs text-muted">
            <Spinner className="h-3.5 w-3.5" />
            加载中…
          </div>
        ) : null}
        {info ? (
          <Card className="divide-y divide-line">
            <div className="flex items-center justify-between px-4 py-3 text-xs">
              <span className="text-muted">名称</span>
              <span className="text-ink">{info.name}</span>
            </div>
            <div className="flex items-center justify-between px-4 py-3 text-xs">
              <span className="text-muted">版本</span>
              <span className="font-mono text-ink">{info.version}</span>
            </div>
            <div className="flex items-center justify-between px-4 py-3 text-xs">
              <span className="text-muted">Schema 版本</span>
              <span className="font-mono text-ink">{info.schemaVersion}</span>
            </div>
            <div className="flex items-center justify-between gap-3 px-4 py-3 text-xs">
              <span className="text-muted">注册表版本</span>
              <span className="font-mono text-ink" title={info.registryVersion}>
                {info.registryVersion}
              </span>
            </div>
            <div className="flex items-center justify-between gap-3 px-4 py-3 text-xs">
              <span className="shrink-0 text-muted">数据库路径</span>
              <span className="flex min-w-0 items-center gap-2">
                <span className="truncate font-mono text-ink" title={info.dbPath}>
                  {info.dbPath}
                </span>
                <Button size="sm" variant="ghost" onClick={copyDbPath} title="复制路径">
                  <CopyIcon className="h-3.5 w-3.5" />
                  {copied ? '已复制' : '复制'}
                </Button>
              </span>
            </div>
            <div className="flex items-center justify-between px-4 py-3 text-xs">
              <span className="text-muted">AI 运行时</span>
              <Badge tone={info.aiEnabled ? 'ok' : 'neutral'}>
                {info.aiEnabled ? '已启用' : '未启用（Phase 6）'}
              </Badge>
            </div>
          </Card>
        ) : null}
      </section>

      <section>
        <h2 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-muted">
          AI 运行时
        </h2>

        {aiSettings.error ? <ErrorNotice error={aiSettings.error} /> : null}

        <Card className="space-y-4 p-5">
          <p className="text-[11px] leading-relaxed text-muted">
            配置 OpenAI 兼容接口（含本地推理服务，如 Ollama / vLLM）。
            「Chat 模型」与「向量模型」均可独立修改；保存后立即对新请求生效，无需重启。
          </p>

          <div className="grid gap-3 sm:grid-cols-2">
            <label className="flex flex-col gap-1.5">
              <span className="text-[11px] font-medium text-muted">API Key</span>
              <Input
                type="password"
                value={aiForm.apiKey}
                onChange={(e) => setAiForm((f) => ({ ...f, apiKey: e.target.value }))}
                placeholder={
                  aiSettings.data?.apiKeySet
                    ? '已配置（输入以替换，或点下方「清除」）'
                    : '未配置，输入以保存'
                }
                autoComplete="off"
              />
            </label>
            <label className="flex flex-col gap-1.5">
              <span className="text-[11px] font-medium text-muted">接口基址</span>
              <Input
                value={aiForm.baseUrl}
                onChange={(e) => setAiForm((f) => ({ ...f, baseUrl: e.target.value }))}
                placeholder="https://api.openai.com/v1"
                autoComplete="off"
              />
            </label>
            <label className="flex flex-col gap-1.5">
              <span className="text-[11px] font-medium text-muted">Chat 模型</span>
              <Input
                value={aiForm.model}
                onChange={(e) => setAiForm((f) => ({ ...f, model: e.target.value }))}
                placeholder="gpt-4o-mini"
                autoComplete="off"
              />
            </label>
            <label className="flex flex-col gap-1.5">
              <span className="text-[11px] font-medium text-muted">向量模型</span>
              <Input
                value={aiForm.embeddingModel}
                onChange={(e) => setAiForm((f) => ({ ...f, embeddingModel: e.target.value }))}
                placeholder="text-embedding-3-small"
                autoComplete="off"
              />
            </label>
            <label className="flex flex-col gap-1.5">
              <span className="text-[11px] font-medium text-muted">上下文预算（token）</span>
              <Input
                type="number"
                min={256}
                step={256}
                value={aiForm.tokenBudget}
                onChange={(e) => setAiForm((f) => ({ ...f, tokenBudget: e.target.value }))}
                placeholder="4000"
                autoComplete="off"
              />
            </label>
          </div>

          {aiError ? <ErrorNotice error={aiError} /> : null}

          <div className="flex flex-wrap items-center gap-2">
            <Button
              type="button"
              variant="primary"
              loading={aiSaving}
              onClick={saveAiSettings}
              disabled={aiSaving}
            >
              保存设置
            </Button>
            {aiSettings.data?.apiKeySet ? (
              <Button type="button" variant="ghost" onClick={clearAiKey} disabled={aiSaving}>
                清除 API Key
              </Button>
            ) : null}
            {aiSaved ? <span className="text-[11px] text-ok">已保存</span> : null}
          </div>

          <p className="text-[11px] leading-relaxed text-muted">
            当前状态：
            <Badge tone={info?.aiEnabled ? 'ok' : 'neutral'} className="mx-1">
              {info?.aiEnabled ? 'AI 已启用' : 'AI 未启用'}
            </Badge>
            保存设置后会即时更新。环境变量（WIKIYA_*）仍可作为默认值，但此处保存的值优先级更高。
          </p>
        </Card>
      </section>

      <section>
        <h2 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-muted">外观</h2>
        <Card className="flex items-center justify-between px-4 py-3">
          <span className="text-xs text-muted">主题</span>
          <div className="flex gap-2">
            <Button
              size="sm"
              variant={theme === 'dark' ? 'primary' : 'secondary'}
              onClick={() => setTheme('dark')}
            >
              暗色
            </Button>
            <Button
              size="sm"
              variant={theme === 'light' ? 'primary' : 'secondary'}
              onClick={() => setTheme('light')}
            >
              亮色
            </Button>
          </div>
        </Card>
      </section>

      <section>
        <h2 className="mb-3 text-[11px] font-semibold uppercase tracking-wider text-muted">Knowledge Health</h2>
        {health.error ? <ErrorNotice error={health.error} /> : null}
        {report ? (
          <>
            <div className="grid grid-cols-2 gap-3 sm:grid-cols-5">
              <Stat label="文档" value={report.totalDocuments} />
              <Stat label="片段" value={report.totalChunks} />
              <Stat label="实体" value={report.totalEntities} />
              <Stat label="Claim" value={report.totalClaims} />
              <Stat label="证据" value={report.totalEvidence} />
            </div>

            <p className="mb-2 mt-5 text-[11px] text-muted">点击指标可跳转到 Review 并带上对应筛选。</p>
            <div className="grid grid-cols-2 gap-3 sm:grid-cols-5">
              {HEALTH_METRICS.map((metric) => (
                <Stat
                  key={metric.key}
                  label={metric.label}
                  value={report[metric.key]}
                  tone={metric.tone}
                  hint={metric.hint}
                  onClick={() => openReview(metric.filter)}
                />
              ))}
            </div>
          </>
        ) : null}
      </section>

      <section>
        <h2 className="mb-1 text-[11px] font-semibold uppercase tracking-wider text-muted">受控词表</h2>
        <p className="mb-3 text-[11px] leading-relaxed text-muted">
          复杂性归系统、不归 UI —— 词表是高级设置的一部分，通常无需关心。这里只做只读浏览。
        </p>

        {registries.error ? <ErrorNotice error={registries.error} /> : null}

        {registries.data ? (
          <Card className="p-4">
            <div className="mb-4 flex flex-wrap gap-2 border-b border-line pb-3">
              {REGISTRY_TABS.map((item) => (
                <button
                  key={item.id}
                  type="button"
                  onClick={() => setTab(item.id)}
                  className={cn(
                    'rounded-md border px-2.5 py-1 text-[11px] font-medium transition-colors',
                    tab === item.id
                      ? 'border-accent/40 bg-accent/10 text-accent'
                      : 'border-line bg-elevated text-muted hover:text-ink',
                  )}
                >
                  {item.label}
                </button>
              ))}
              <span className="ml-auto self-center font-mono text-[10px] text-muted">
                {registries.data.registryVersion}
              </span>
            </div>

            <RegistryContent tab={tab} registries={registries.data} />
          </Card>
        ) : null}
      </section>
    </div>
  );
}
