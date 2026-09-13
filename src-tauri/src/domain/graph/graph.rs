//! 从某个实体出发的有界邻域探索。

use std::collections::{HashMap, HashSet, VecDeque};

/// 默认展开深度。
pub const DEFAULT_DEPTH: usize = 1;
/// 深度上限：再深就不是"看邻域"，而是"看全图"了。
pub const MAX_DEPTH: usize = 3;
/// 节点数量上限。
///
/// 有它才能兑现「避免默认加载全图」的承诺：即使某个实体是超级枢纽
/// （比如 "Rust"），界面也只加载前 120 个节点，而不是把浏览器卡死。
pub const MAX_NODES: usize = 120;

/// 图节点。
#[derive(Debug, Clone, PartialEq)]
pub struct GraphNode {
    pub id: String,
    pub name: String,
    pub type_name: String,
    pub status: String,
    /// 距离起点的跳数（起点为 0）。
    pub depth: usize,
}

/// 图边。`predicate` 是**从 source 看过去**的说法。
#[derive(Debug, Clone, PartialEq)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub predicate: String,
}

/// 节点种子（由 Repository 提供的中性数据，避免 Domain 依赖数据库行）。
#[derive(Debug, Clone)]
pub struct NodeSeed {
    pub id: String,
    pub name: String,
    pub type_name: String,
    pub status: String,
}

/// 一次探索的结果。
#[derive(Debug, Clone, Default)]
pub struct Neighborhood {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    /// 是否因为达到节点上限而被截断（UI 需要如实告知用户）。
    pub truncated: bool,
}

/// 从 `root` 出发做广度优先探索。
///
/// 遍历时把边当作**无向**：从被指向的一方看过去，关系依然存在
/// （`wiki-ya uses SQLite` 对 SQLite 而言是 `used_by`）。
/// 只沿一个方向走会让图谱的一半不可达。
///
/// 注意：边的**存储**不写反向冗余行，反向只在展示时由
/// `inverse_label` 生成，因此这里不会出现重复边。
pub fn explore(
    root: &str,
    nodes: &[NodeSeed],
    edges: &[GraphEdge],
    depth: usize,
    predicate_filter: Option<&[String]>,
) -> Neighborhood {
    let depth = depth.clamp(1, MAX_DEPTH);
    let by_id: HashMap<&str, &NodeSeed> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    let mut adjacency: HashMap<&str, Vec<&GraphEdge>> = HashMap::new();
    for edge in edges {
        if let Some(filter) = predicate_filter {
            if !filter.is_empty() && !filter.iter().any(|p| p == &edge.predicate) {
                continue;
            }
        }
        adjacency
            .entry(edge.source.as_str())
            .or_default()
            .push(edge);
        adjacency
            .entry(edge.target.as_str())
            .or_default()
            .push(edge);
    }

    let mut result = Neighborhood::default();
    let mut visited: HashSet<&str> = HashSet::new();
    let mut queue: VecDeque<(&str, usize)> = VecDeque::new();

    if !by_id.contains_key(root) {
        return result;
    }

    visited.insert(root);
    queue.push_back((root, 0));

    while let Some((current, current_depth)) = queue.pop_front() {
        if let Some(seed) = by_id.get(current) {
            result.nodes.push(GraphNode {
                id: seed.id.clone(),
                name: seed.name.clone(),
                type_name: seed.type_name.clone(),
                status: seed.status.clone(),
                depth: current_depth,
            });
        }

        if current_depth >= depth {
            continue;
        }

        for edge in adjacency.get(current).cloned().unwrap_or_default() {
            let neighbour = if edge.source == current {
                edge.target.as_str()
            } else {
                edge.source.as_str()
            };
            if !visited.insert(neighbour) {
                continue;
            }
            if result.nodes.len() >= MAX_NODES {
                result.truncated = true;
                continue;
            }
            queue.push_back((neighbour, current_depth + 1));
        }
    }

    // 只保留两端都出现在结果里的边，避免前端渲染出悬空连线。
    let present: HashSet<&str> = result.nodes.iter().map(|n| n.id.as_str()).collect();
    let mut seen_edges: HashSet<(&str, &str, &str)> = HashSet::new();
    for edge in edges {
        if !present.contains(edge.source.as_str()) || !present.contains(edge.target.as_str()) {
            continue;
        }
        if let Some(filter) = predicate_filter {
            if !filter.is_empty() && !filter.iter().any(|p| p == &edge.predicate) {
                continue;
            }
        }
        let key = (
            edge.source.as_str(),
            edge.target.as_str(),
            edge.predicate.as_str(),
        );
        if seen_edges.insert(key) {
            result.edges.push(edge.clone());
        }
    }

    // 起点在前，其余按跳数、名称排序：布局稳定性对"看得懂"很重要。
    result.nodes.sort_by(|left, right| {
        left.depth
            .cmp(&right.depth)
            .then_with(|| left.name.cmp(&right.name))
    });

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeds() -> Vec<NodeSeed> {
        ["a", "b", "c", "d", "e"]
            .iter()
            .map(|id| NodeSeed {
                id: (*id).to_string(),
                name: (*id).to_uppercase(),
                type_name: "Software".to_string(),
                status: "candidate".to_string(),
            })
            .collect()
    }

    fn edge(source: &str, target: &str, predicate: &str) -> GraphEdge {
        GraphEdge {
            source: source.to_string(),
            target: target.to_string(),
            predicate: predicate.to_string(),
        }
    }

    #[test]
    fn unknown_root_yields_an_empty_neighbourhood() {
        let result = explore("zzz", &seeds(), &[], 1, None);
        assert!(result.nodes.is_empty());
        assert!(result.edges.is_empty());
    }

    #[test]
    fn depth_one_returns_the_root_and_its_direct_neighbours() {
        let edges = vec![edge("a", "b", "uses"), edge("b", "c", "uses")];
        let result = explore("a", &seeds(), &edges, 1, None);
        let ids: Vec<&str> = result.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
        assert!(!result.truncated);
    }

    #[test]
    fn depth_two_reaches_the_second_ring() {
        let edges = vec![edge("a", "b", "uses"), edge("b", "c", "uses")];
        let result = explore("a", &seeds(), &edges, 2, None);
        let ids: Vec<&str> = result.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }

    #[test]
    fn traversal_follows_edges_in_both_directions() {
        // b 只作为 target 出现，从 b 出发依然应该到达 a。
        let edges = vec![edge("a", "b", "uses")];
        let result = explore("b", &seeds(), &edges, 1, None);
        let ids: Vec<&str> = result.nodes.iter().map(|n| n.id.as_str()).collect();
        // 根节点必须在最前，邻居紧随其后（顺序由遍历决定，这里只校验可达性）。
        assert_eq!(ids[0], "b");
        assert!(ids.contains(&"a"));
    }

    #[test]
    fn predicate_filter_narrows_the_neighbourhood() {
        let edges = vec![edge("a", "b", "uses"), edge("a", "c", "depends_on")];
        let filter = vec!["uses".to_string()];
        let result = explore("a", &seeds(), &edges, 1, Some(&filter));
        let ids: Vec<&str> = result.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[test]
    fn depth_is_clamped_to_the_maximum() {
        let edges = vec![
            edge("a", "b", "uses"),
            edge("b", "c", "uses"),
            edge("c", "d", "uses"),
            edge("d", "e", "uses"),
        ];
        let result = explore("a", &seeds(), &edges, 99, None);
        assert!(result.nodes.iter().all(|n| n.depth <= MAX_DEPTH));
        assert!(!result.nodes.iter().any(|n| n.id == "e"));
    }

    #[test]
    fn dangling_nodes_are_dropped_from_the_result() {
        // 边指向一个不存在的节点：节点缺失，边也必须被剔除。
        let edges = vec![edge("a", "ghost", "uses")];
        let result = explore("a", &seeds(), &edges, 2, None);
        assert_eq!(result.nodes.len(), 1);
        assert!(result.edges.is_empty());
    }

    #[test]
    fn duplicate_edges_are_collapsed() {
        let edges = vec![edge("a", "b", "uses"), edge("a", "b", "uses")];
        let result = explore("a", &seeds(), &edges, 1, None);
        assert_eq!(result.edges.len(), 1);
    }
}
