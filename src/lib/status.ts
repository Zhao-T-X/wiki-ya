/**
 * 受控词表 → 展示层的映射。
 *
 * 取值来源：`docs/领域枚举与不变量定义.md`。这里只负责「中文标签 + 语义色调」，
 * 不做任何归一化或兜底逻辑——未知取值原样展示并采用中性色，避免掩盖后端问题。
 */

export type Tone = 'neutral' | 'accent' | 'ok' | 'warn' | 'danger';

const STATUS_LABELS: Record<string, string> = {
  // ClaimStatus
  draft: '草稿',
  candidate: '候选',
  verified: '已确认',
  rejected: '已拒绝',
  archived: '已归档',
  superseded: '已取代',
  // ClaimRelationStatus
  accepted: '已接受',
  // EntityStatus / RelationStatus 复用上面同名值
  // IdeaStatus
  implemented: '已落地',
  // QuestionStatus
  open: '开放',
  answered: '已回答',
  partially_answered: '部分回答',
  resolved: '已解决',
  // EventStatus
  planned: '计划中',
  ongoing: '进行中',
  completed: '已完成',
  cancelled: '已取消',
  failed: '失败',
  unknown: '未知',
  // ResearchTaskStatus
  running: '运行中',
};

const STATUS_TONES: Record<string, Tone> = {
  draft: 'neutral',
  candidate: 'warn',
  verified: 'ok',
  rejected: 'danger',
  archived: 'neutral',
  superseded: 'neutral',
  accepted: 'ok',
  implemented: 'ok',
  open: 'accent',
  answered: 'ok',
  partially_answered: 'warn',
  resolved: 'ok',
  planned: 'neutral',
  ongoing: 'accent',
  completed: 'ok',
  cancelled: 'neutral',
  failed: 'danger',
  unknown: 'neutral',
  running: 'accent',
};

export function statusLabel(status: string): string {
  return STATUS_LABELS[status] ?? status;
}

export function statusTone(status: string): Tone {
  return STATUS_TONES[status] ?? 'neutral';
}

const RELATIONSHIP_LABELS: Record<string, string> = {
  duplicate: '重复',
  coexists: '并存',
  supplements: '补充',
  supersedes: '取代',
  contradicts: '冲突',
  unclear: '待定',
};

const RELATIONSHIP_TONES: Record<string, Tone> = {
  duplicate: 'warn',
  coexists: 'accent',
  supplements: 'accent',
  supersedes: 'ok',
  contradicts: 'danger',
  unclear: 'neutral',
};

export function relationshipLabel(relationship: string): string {
  return RELATIONSHIP_LABELS[relationship] ?? relationship;
}

export function relationshipTone(relationship: string): Tone {
  return RELATIONSHIP_TONES[relationship] ?? 'neutral';
}

const CLAIM_TYPE_LABELS: Record<string, string> = {
  factual: '事实',
  definitional: '定义',
  causal: '因果',
  comparative: '比较',
  evaluative: '评价',
  predictive: '预测',
  normative: '规范',
  hypothetical: '假设',
};

export function claimTypeLabel(claimType: string): string {
  return CLAIM_TYPE_LABELS[claimType] ?? claimType;
}

const MODALITY_LABELS: Record<string, string> = {
  asserted: '断言',
  possible: '可能',
  probable: '很可能',
  capable: '有能力',
  necessary: '必要',
  recommended: '建议',
};

export function modalityLabel(modality: string): string {
  return MODALITY_LABELS[modality] ?? modality;
}

export function polarityLabel(polarity: string): string {
  if (polarity === 'positive') return '肯定';
  if (polarity === 'negative') return '否定';
  return polarity;
}

/** SearchHit.kind → 中文标签。 */
const KIND_LABELS: Record<string, string> = {
  document: '文档',
  chunk: '片段',
  claim: 'Claim',
  entity: '实体',
};

export function searchKindLabel(kind: string): string {
  return KIND_LABELS[kind] ?? kind;
}

/** ClaimCard.lifecycle → 中文标签。excluded（被排除）与 superseded（历史）必须区分。 */
const LIFECYCLE_LABELS: Record<string, string> = {
  current: '当前知识',
  superseded: '历史知识',
  excluded: '不参与当前知识',
};

export function lifecycleLabel(lifecycle: string): string {
  return LIFECYCLE_LABELS[lifecycle] ?? lifecycle;
}
