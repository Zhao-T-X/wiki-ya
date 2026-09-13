import type { SearchHit } from '@/types/ipc';

/**
 * 把搜索结果映射到可跳转的路由。
 * 保持单一映射来源，避免 Search / CommandPalette 各自实现导致漂移。
 */
export function hitTarget(hit: SearchHit): string | null {
  switch (hit.kind) {
    case 'entity':
      return `/knowledge/${hit.id}`;
    case 'claim':
      return `/claims/${hit.claimId ?? hit.id}`;
    case 'document':
      return `/documents/${hit.documentId ?? hit.id}`;
    case 'chunk':
      return hit.documentId ? `/documents/${hit.documentId}` : null;
    default:
      return null;
  }
}
