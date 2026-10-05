import { useState } from 'react';

import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Card } from '@/components/ui/Card';
import { ErrorNotice } from '@/components/ErrorNotice';
import { Input } from '@/components/ui/Input';
import {
  create_agent_profile,
  delete_agent_profile,
  list_agent_profiles,
  list_skills,
  update_agent_profile,
  WikiError,
} from '@/lib/api';
import { useAsyncData } from '@/lib/hooks';

interface EditingProfile {
  name: string;
  displayName: string;
  skills: string[];
  policy: string[];
  isNew: boolean;
}

/**
 * Agent 管理面板（M12：自定义 Agent）。
 *
 * 用户组合 Agent = 名字 + Skills + Policy（上限 propose——结构上
 * 拿不到 MUTATE，改知识永远经人类 Review）。
 */
export function AgentManagerPanel() {
  const profiles = useAsyncData(() => list_agent_profiles(), []);
  const skills = useAsyncData(() => list_skills(), []);
  const [editing, setEditing] = useState<EditingProfile | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<WikiError | null>(null);

  function refresh() {
    profiles.reload();
  }

  function startCreate() {
    setEditing({
      name: '',
      displayName: '',
      skills: ['knowledge-answering'],
      policy: ['read'],
      isNew: true,
    });
  }

  function startEdit(name: string, displayName: string, skills: string[], policy: string[]) {
    setEditing({ name, displayName, skills, policy, isNew: false });
  }

  function toggleSkill(skill: string) {
    setEditing((prev) => {
      if (!prev) return prev;
      const has = prev.skills.includes(skill);
      return {
        ...prev,
        skills: has ? prev.skills.filter((s) => s !== skill) : [...prev.skills, skill],
      };
    });
  }

  function setPolicyCap(cap: 'read' | 'propose') {
    setEditing((prev) => {
      if (!prev) return prev;
      return { ...prev, policy: cap === 'propose' ? ['read', 'propose'] : ['read'] };
    });
  }

  async function save() {
    if (!editing || busy) return;
    setBusy(true);
    setError(null);
    try {
      if (editing.isNew) {
        await create_agent_profile({
          name: editing.name,
          displayName: editing.displayName,
          skills: editing.skills,
          policy: editing.policy,
        });
      } else {
        await update_agent_profile({
          name: editing.name,
          displayName: editing.displayName,
          skills: editing.skills,
          policy: editing.policy,
        });
      }
      setEditing(null);
      refresh();
    } catch (cause: unknown) {
      setError(cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause)));
    } finally {
      setBusy(false);
    }
  }

  async function remove(name: string) {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await delete_agent_profile({ id: name });
      refresh();
    } catch (cause: unknown) {
      setError(cause instanceof WikiError ? cause : new WikiError('INTERNAL_ERROR', String(cause)));
    } finally {
      setBusy(false);
    }
  }

  const allSkills = skills.data ?? [];

  return (
    <div className="space-y-3">
      {error ? <ErrorNotice error={error} /> : null}

      {profiles.data?.map((profile) => (
        <Card key={profile.name} className="p-4">
          <div className="flex items-start justify-between gap-3">
            <div className="min-w-0 space-y-1">
              <div className="flex flex-wrap items-center gap-1.5">
                <span className="text-sm font-medium text-ink">{profile.displayName}</span>
                <Badge tone="neutral">{profile.name}</Badge>
                {profile.policy.includes('propose') ? (
                  <Badge tone="warn">可提案</Badge>
                ) : (
                  <Badge tone="ok">只读</Badge>
                )}
              </div>
              <p className="text-meta text-muted">
                Skills：{profile.skills.length > 0 ? profile.skills.join(' · ') : '（无）'}
              </p>
            </div>
            <div className="flex shrink-0 gap-1.5">
              <Button
                size="sm"
                variant="ghost"
                onClick={() =>
                  startEdit(profile.name, profile.displayName, profile.skills, profile.policy)
                }
              >
                编辑
              </Button>
              {profile.name !== 'knowledge-analyst' ? (
                <Button size="sm" variant="ghost" disabled={busy} onClick={() => remove(profile.name)}>
                  删除
                </Button>
              ) : null}
            </div>
          </div>
        </Card>
      ))}

      {!editing ? (
        <Button size="sm" variant="secondary" onClick={startCreate}>
          创建 Agent
        </Button>
      ) : (
        <Card className="p-4">
          <div className="space-y-3">
            {editing.isNew ? (
              <label className="block space-y-1">
                <span className="text-meta font-medium text-muted">标识名（小写字母/数字/连字符）</span>
                <Input
                  value={editing.name}
                  onChange={(e) => setEditing({ ...editing, name: e.target.value })}
                  placeholder="technical-researcher"
                />
              </label>
            ) : null}
            <label className="block space-y-1">
              <span className="text-meta font-medium text-muted">展示名</span>
              <Input
                value={editing.displayName}
                onChange={(e) => setEditing({ ...editing, displayName: e.target.value })}
                placeholder="技术研究员"
              />
            </label>
            <div className="space-y-1">
              <span className="text-meta font-medium text-muted">Skills（按顺序执行）</span>
              <div className="flex flex-wrap gap-2">
                {allSkills.map((skill) => {
                  const checked = editing.skills.includes(skill.name);
                  return (
                    <label
                      key={skill.name}
                      className={`flex cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 py-1.5 text-meta ${
                        checked ? 'border-accent bg-accent/10 text-ink' : 'border-line text-muted'
                      }`}
                    >
                      <input
                        type="checkbox"
                        checked={checked}
                        onChange={() => toggleSkill(skill.name)}
                        className="accent-accent"
                      />
                      {skill.name}
                    </label>
                  );
                })}
              </div>
            </div>
            <div className="space-y-1">
              <span className="text-meta font-medium text-muted">权限上限</span>
              <div className="flex gap-2">
                <label
                  className={`flex cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 py-1.5 text-meta ${
                    !editing.policy.includes('propose')
                      ? 'border-accent bg-accent/10 text-ink'
                      : 'border-line text-muted'
                  }`}
                >
                  <input
                    type="radio"
                    checked={!editing.policy.includes('propose')}
                    onChange={() => setPolicyCap('read')}
                    className="accent-accent"
                  />
                  只读（READ）
                </label>
                <label
                  className={`flex cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 py-1.5 text-meta ${
                    editing.policy.includes('propose')
                      ? 'border-accent bg-accent/10 text-ink'
                      : 'border-line text-muted'
                  }`}
                >
                  <input
                    type="radio"
                    checked={editing.policy.includes('propose')}
                    onChange={() => setPolicyCap('propose')}
                    className="accent-accent"
                  />
                  可提案（PROPOSE）
                </label>
              </div>
              <p className="text-[10px] text-muted/70">
                改知识永远经人类 Review——Agent 拿不到 MUTATE 权限。
              </p>
            </div>
            <div className="flex gap-2">
              <Button size="sm" variant="primary" loading={busy} onClick={save}>
                {editing.isNew ? '创建' : '保存'}
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setEditing(null)}>
                取消
              </Button>
            </div>
          </div>
        </Card>
      )}
    </div>
  );
}
