import type { AskSource, SearchHit } from '@/types/ipc';

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

/**
 * 把答案引用（`AskSource`）映射到可跳转的路由。
 *
 * 与 [`hitTarget`] 同文件的原因：二者是**同一套 kind → 路由**映射。此前
 * `SearchPage` / `AskPage` 各抄了一份 `sourcePath`，改路由得改两处。
 *
 * `chunk` 故意落回 `null`：切片没有独立详情页，而 `/documents/:id` 也不是它的
 * 详情页——给一个会跳错的链接比不给更糟。引用照常显示标题，只是不带链接。
 */
export function sourceTarget(source: AskSource): string | null {
  switch (source.kind) {
    case 'entity':
      return `/knowledge/${source.id}`;
    case 'claim':
      return `/claims/${source.id}`;
    case 'document':
      return `/documents/${source.id}`;
    default:
      return null;
  }
}
