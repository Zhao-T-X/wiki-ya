import { useState } from 'react';

import { DatabaseIcon } from '@/components/icons';
import { ErrorNotice } from '@/components/ErrorNotice';
import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { Input } from '@/components/ui/Input';
import { Spinner } from '@/components/ui/Spinner';
import { PageHeader } from '@/components/PageHeader';
import { WikiError, probe_migration, run_migration } from '@/lib/api';
import type { MigrationProbe, MigrationReport } from '@/types/ipc';

/**
 * 存量库迁移（Phase 8）：只读探测 → 备份目标库 → 导入（documents → claims，幂等可重跑）。
 *
 * 诚实边界：迁移前先备份，导入的任何跳过/失败都带原因进 `notes`，绝不静默丢数据。
 */
export function MigrationPage() {
  const [sourcePath, setSourcePath] = useState('');
  const [probing, setProbing] = useState(false);
  const [probe, setProbe] = useState<MigrationProbe | null>(null);
  const [probeError, setProbeError] = useState<WikiError | null>(null);

  const [migrating, setMigrating] = useState(false);
  const [report, setReport] = useState<MigrationReport | null>(null);
  const [migrateError, setMigrateError] = useState<WikiError | null>(null);

  async function handleProbe() {
    const path = sourcePath.trim();
    if (!path || probing) return;
    setProbing(true);
    setProbe(null);
    setReport(null);
    setProbeError(null);
    try {
      setProbe(await probe_migration({ sourcePath: path }));
    } catch (cause: unknown) {
      setProbeError(
        cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause)),
      );
    } finally {
      setProbing(false);
    }
  }

  async function handleRun() {
    const path = sourcePath.trim();
    if (!path || migrating) return;
    setMigrating(true);
    setReport(null);
    setMigrateError(null);
    try {
      const result = await run_migration({ sourcePath: path });
      setReport(result);
      setProbe(null); // 重新探测以反映最新状态
    } catch (cause: unknown) {
      setMigrateError(
        cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause)),
      );
    } finally {
      setMigrating(false);
    }
  }

  return (
    <div className="space-y-6">
      <PageHeader
        title="Migration"
        subtitle="从存量库（SQLite）迁移文档与 Claim。迁移前会自动备份当前库，导入幂等可重跑。"
      />

      <Card className="space-y-4 p-5">
        <label className="flex flex-col gap-1.5">
          <span className="text-[11px] font-medium text-muted">存量库路径</span>
          <Input
            value={sourcePath}
            onChange={(e) => setSourcePath(e.target.value)}
            placeholder="/path/to/personal-wiki.db"
            autoComplete="off"
          />
        </label>

        <div className="flex flex-wrap items-center gap-2">
          <Button type="button" variant="primary" onClick={handleProbe} loading={probing} disabled={!sourcePath.trim()}>
            探测
          </Button>
          <Button
            type="button"
            variant="primary"
            onClick={handleRun}
            loading={migrating}
            disabled={!sourcePath.trim() || migrating}
          >
            备份并导入
          </Button>
        </div>

        {probeError ? <ErrorNotice error={probeError} /> : null}
        {migrateError ? <ErrorNotice error={migrateError} /> : null}
      </Card>

      {probing ? (
        <div className="flex items-center gap-2 text-xs text-muted">
          <Spinner className="h-3.5 w-3.5" />
          正在只读探测存量库…
        </div>
      ) : null}

      {probe ? (
        <Card className="space-y-3 p-5">
          <div className="flex items-center gap-2">
            <Badge tone={probe.compatible ? 'ok' : 'danger'}>
              {probe.compatible ? '兼容' : '不兼容'}
            </Badge>
            <span className="truncate font-mono text-[11px] text-muted">{probe.sourcePath}</span>
          </div>
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
            <Metric label="文档" value={probe.documentCount} />
            <Metric label="Claim" value={probe.claimCount} />
            <Metric label="documents 表" value={probe.hasDocuments ? '有' : '无'} />
            <Metric label="claims 表" value={probe.hasClaims ? '有' : '无'} />
          </div>
          {probe.tables.length > 0 ? (
            <p className="text-[11px] text-muted">
              发现表：{probe.tables.join(', ')}
            </p>
          ) : null}
          {!probe.compatible ? (
            <p className="text-[11px] leading-relaxed text-warn">
              存量库缺少 documents / entities / claims 三张表，无法迁移。请确认路径指向正确的源库。
            </p>
          ) : null}
        </Card>
      ) : null}

      {migrating ? (
        <div className="flex items-center gap-2 text-xs text-muted">
          <Spinner className="h-3.5 w-3.5" />
          正在备份并导入，请稍候…
        </div>
      ) : null}

      {report ? (
        <Card className="space-y-3 p-5">
          <div className="flex items-center gap-2">
            <DatabaseIcon className="h-4 w-4 text-ok" />
            <span className="text-sm font-medium text-ink">迁移完成</span>
          </div>
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
            <Metric label="文档导入" value={report.documentsImported} />
            <Metric label="文档跳过" value={report.documentsSkipped} />
            <Metric label="Claim 导入" value={report.claimsImported} />
            <Metric label="Claim 跳过" value={report.claimsSkipped} />
          </div>
          {report.backupPath ? (
            <p className="text-[11px] text-muted">
              备份：<span className="font-mono">{report.backupPath}</span>
            </p>
          ) : null}
          {report.notes.length > 0 ? (
            <div>
              <p className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-muted">
                跳过 / 失败（{report.notes.length}）
              </p>
              <ul className="space-y-1">
                {report.notes.map((note, index) => (
                  <li key={index} className="text-[11px] leading-relaxed text-warn">
                    · {note}
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
        </Card>
      ) : null}
    </div>
  );
}

function Metric({ label, value }: { label: string; value: number | string }) {
  return (
    <div className="rounded-lg border border-line bg-canvas px-3 py-2">
      <p className="text-[10px] text-muted">{label}</p>
      <p className="mt-0.5 text-sm text-ink">{value}</p>
    </div>
  );
}
