import { EmptyState } from '@/components/ui/EmptyState';
import { GraphIcon } from '@/components/icons';
import { cn } from '@/lib/cn';
import { truncate } from '@/lib/format';
import type { GraphEdge, GraphNode } from '@/types/ipc';

export interface GraphCanvasProps {
  nodes: GraphNode[];
  edges: GraphEdge[];
  /** 根节点 id：由它决定径向布局中心。 */
  activeId?: string | null;
  onSelect?: (id: string) => void;
  height?: number;
}

interface Point {
  x: number;
  y: number;
}

/** 无向邻接表。 */
function buildAdjacency(edges: GraphEdge[]): Map<string, string[]> {
  const map = new Map<string, string[]>();
  const add = (from: string, to: string) => {
    const list = map.get(from);
    if (list) list.push(to);
    else map.set(from, [to]);
  };
  for (const edge of edges) {
    add(edge.source, edge.target);
    add(edge.target, edge.source);
  }
  return map;
}

/** BFS 分层（当后端未提供有意义的 depth 时的确定性回退）。 */
function bfsDepth(rootId: string, adjacency: Map<string, string[]>): Map<string, number> {
  const depths = new Map<string, number>([[rootId, 0]]);
  const queue: string[] = [rootId];
  while (queue.length > 0) {
    const current = queue.shift();
    if (current === undefined) break;
    const depth = depths.get(current) ?? 0;
    for (const next of adjacency.get(current) ?? []) {
      if (!depths.has(next)) {
        depths.set(next, depth + 1);
        queue.push(next);
      }
    }
  }
  return depths;
}

/**
 * 确定性径向布局：按 depth 分层，depth 0 居中，其余环状均分。
 * 不引入 d3 / 力导向库，不做全图渲染（技术设计文档 §67：避免默认加载全图）。
 */
export function GraphCanvas({ nodes, edges, activeId, onSelect, height = 420 }: GraphCanvasProps) {
  if (nodes.length === 0) {
    return (
      <EmptyState
        title="暂无邻域数据"
        description="该实体在当前 depth 与 predicate 筛选下没有可展示的邻居。"
        icon={<GraphIcon className="h-5 w-5" />}
      />
    );
  }

  const rootId = activeId && nodes.some((node) => node.id === activeId) ? activeId : (nodes[0]?.id ?? '');
  const providedDepths = new Set(nodes.map((node) => node.depth));
  const useProvided = providedDepths.size > 1 || (providedDepths.size === 1 && !providedDepths.has(0));
  const bfs = bfsDepth(rootId, buildAdjacency(edges));

  const depthOf = (node: GraphNode): number =>
    useProvided ? node.depth : (bfs.get(node.id) ?? 0);

  const layers = new Map<number, GraphNode[]>();
  for (const node of nodes) {
    const depth = depthOf(node);
    const list = layers.get(depth);
    if (list) list.push(node);
    else layers.set(depth, [node]);
  }

  const sortedDepths = [...layers.keys()].sort((a, b) => a - b);
  const ringRadius = (depth: number): number => (depth === 0 ? 0 : 96 + (depth - 1) * 118);

  const positions = new Map<string, Point>();
  for (const depth of sortedDepths) {
    const layerNodes = layers.get(depth) ?? [];
    const radius = ringRadius(depth);
    const count = layerNodes.length;
    layerNodes.forEach((node, index) => {
      if (depth === 0) {
        positions.set(node.id, { x: 0, y: 0 });
        return;
      }
      const angle = count === 1 ? -Math.PI / 2 : -Math.PI / 2 + (index * 2 * Math.PI) / count;
      positions.set(node.id, { x: radius * Math.cos(angle), y: radius * Math.sin(angle) });
    });
  }

  const maxRadius = sortedDepths.reduce((acc, depth) => Math.max(acc, ringRadius(depth)), 0);
  const extent = Math.max(180, maxRadius + 110);

  return (
    <div className="overflow-hidden rounded-xl border border-line bg-canvas" style={{ height }}>
      <svg
        viewBox={`${-extent} ${-extent} ${extent * 2} ${extent * 2}`}
        preserveAspectRatio="xMidYMid meet"
        className="h-full w-full"
        role="img"
        aria-label="实体邻域图"
      >
        <g>
          {edges.map((edge, index) => {
            const from = positions.get(edge.source);
            const to = positions.get(edge.target);
            if (!from || !to) return null;
            return (
              <line
                key={`${edge.source}-${edge.target}-${edge.predicate}-${index}`}
                x1={from.x}
                y1={from.y}
                x2={to.x}
                y2={to.y}
                stroke="rgb(var(--wy-line))"
                strokeWidth={1}
              />
            );
          })}
        </g>

        {nodes.map((node) => {
          const point = positions.get(node.id);
          if (!point) return null;
          const isRoot = node.id === rootId;
          const radius = depthOf(node) === 0 ? 11 : 8;
          return (
            <g
              key={node.id}
              transform={`translate(${point.x} ${point.y})`}
              onClick={() => onSelect?.(node.id)}
              className={cn(onSelect ? 'cursor-pointer' : 'cursor-default')}
            >
              <circle
                r={radius}
                fill={isRoot ? 'rgb(var(--wy-accent))' : 'rgb(var(--wy-elevated))'}
                stroke={isRoot ? 'rgb(var(--wy-accent))' : 'rgb(var(--wy-line))'}
                strokeWidth={1.5}
              />
              <text
                y={radius + 14}
                textAnchor="middle"
                fontSize={11}
                fill={isRoot ? 'rgb(var(--wy-ink))' : 'rgb(var(--wy-muted))'}
              >
                {truncate(node.name, 14)}
              </text>
            </g>
          );
        })}
      </svg>
    </div>
  );
}
