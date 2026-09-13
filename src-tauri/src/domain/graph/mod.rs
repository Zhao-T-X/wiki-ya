//! Graph —— 实体邻域的探索语义。
//!
//! PRD §25：Graph **不是默认首页**，也不默认加载全图。
//! 它只服务一个路径：`Entity Detail → Related Knowledge → Explore Graph`。
//! 所以这里没有"全图"这个概念，只有从某个实体出发、有界的邻域。

pub mod graph;

pub use graph::{
    explore, GraphEdge, GraphNode, Neighborhood, NodeSeed, DEFAULT_DEPTH, MAX_DEPTH, MAX_NODES,
};
