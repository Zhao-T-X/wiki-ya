import { Badge } from '@/components/ui/Badge';
import { formatChars, formatRelativeTime } from '@/lib/format';
import type { DocumentSummary } from '@/types/ipc';

export interface DocumentCardProps {
  doc: DocumentSummary;
  onOpen?: (id: string) => void;
}

/** 文档卡片：展示捕获结果（切分片数、字数、来源、时间）。 */
export function DocumentCard({ doc, onOpen }: DocumentCardProps) {
  const interactive = Boolean(onOpen);

  return (
    <button
      type="button"
      disabled={!interactive}
      onClick={() => onOpen?.(doc.id)}
      className="group w-full rounded-xl border border-line bg-surface p-4 text-left transition-colors hover:border-muted/40 hover:bg-elevated disabled:cursor-default disabled:hover:border-line disabled:hover:bg-surface"
    >
      <div className="flex items-start justify-between gap-3">
        <h3 className="truncate text-sm font-medium text-ink">{doc.title}</h3>
        <Badge>{doc.sourceType}</Badge>
      </div>
      <div className="mt-2 flex flex-wrap items-center gap-x-2.5 gap-y-1 text-[11px] text-muted">
        <span>{formatRelativeTime(doc.createdAt)}</span>
        <span className="text-line">·</span>
        <span>{doc.chunkCount} 个片段</span>
        <span className="text-line">·</span>
        <span>{formatChars(doc.charCount)}</span>
      </div>
    </button>
  );
}
