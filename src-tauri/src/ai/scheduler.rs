//! AI 后台任务并发闸门（PERF-05）。
//!
//! 背景：抽取、抽取型 Skill、研究各自 `spawn_blocking`，彼此不知情。同时跑多个
//! 会叠加出：API 限流、CPU 抖动、SQLite 写锁争用、内存上升、UI 互相抢资源。
//!
//! 产品是**单用户 Local-first**：目标不是并发最大化，而是**整机响应稳定**。
//! 所以这里给每类任务一个明确上限，而不是让它们各自为战。
//!
//! 闸门只放在**命令/编排层**（async 命令才能 await）：服务层保持纯业务、
//! 不感知并发，单元测试服务时也不会被信号量干扰。

use std::sync::{Arc, OnceLock};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// 抽取并发上限。最重的一类：多次模型调用 + embedding + 大量 DB 写。
const EXTRACTION_CAPACITY: usize = 2;
/// 抽取型 Skill 并发上限（与抽取争抢同一批端点配额）。
const SKILL_CAPACITY: usize = 2;
/// 研究 / 多轮 Agent 并发上限。
///
/// 研究比抽取更贵：单次任务内部就是多轮 ReAct（每轮一次模型调用 + 可能一次
/// 工具调用），端点配额与 CPU 压力都远大于单轮补全。
const AGENT_CAPACITY: usize = 2;

/// 抽取闸门。
pub fn extraction_gate() -> &'static Arc<Semaphore> {
    static GATE: OnceLock<Arc<Semaphore>> = OnceLock::new();
    GATE.get_or_init(|| Arc::new(Semaphore::new(EXTRACTION_CAPACITY)))
}

/// 抽取型 Skill 闸门。
pub fn skill_gate() -> &'static Arc<Semaphore> {
    static GATE: OnceLock<Arc<Semaphore>> = OnceLock::new();
    GATE.get_or_init(|| Arc::new(Semaphore::new(SKILL_CAPACITY)))
}

/// 取一个抽取许可。
///
/// `None` 只在闸门被关闭时出现（`Semaphore::close`），此时调用方应放弃执行
/// 而不是继续跑——闸门都关了还干活显然不是调用方的本意。
pub async fn acquire_extraction() -> Option<OwnedSemaphorePermit> {
    extraction_gate().clone().acquire_owned().await.ok()
}

/// 取一个 Skill 许可（语义同 [`acquire_extraction`]）。
pub async fn acquire_skill() -> Option<OwnedSemaphorePermit> {
    skill_gate().clone().acquire_owned().await.ok()
}

/// 研究 / 多轮 Agent 闸门。
pub fn agent_gate() -> &'static Arc<Semaphore> {
    static GATE: OnceLock<Arc<Semaphore>> = OnceLock::new();
    GATE.get_or_init(|| Arc::new(Semaphore::new(AGENT_CAPACITY)))
}

/// 取一个研究 / Agent 许可（语义同 [`acquire_extraction`]）。
pub async fn acquire_agent() -> Option<OwnedSemaphorePermit> {
    agent_gate().clone().acquire_owned().await.ok()
}

/// 当前排队/占用的抽取数（诊断用：UI/日志可观测闸门压力）。
pub async fn extraction_in_flight() -> usize {
    EXTRACTION_CAPACITY - extraction_gate().available_permits()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 闸门是**进程级全局**状态，因此相关断言必须放在同一个测试里顺序执行——
    /// 拆成多个 `#[tokio::test]` 会在并行运行时互相抢占许可而随机失败。
    #[tokio::test]
    async fn extraction_gate_limits_then_releases() {
        // 上限内：可同时取到 EXTRACTION_CAPACITY 个许可。
        let a = acquire_extraction().await;
        let b = acquire_extraction().await;
        assert!(a.is_some() && b.is_some(), "上限内应拿到许可");
        assert_eq!(extraction_in_flight().await, EXTRACTION_CAPACITY);

        // 超出上限：应处于**排队等待**（而不是立即失败）。
        let overflow = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            acquire_extraction(),
        )
        .await;
        assert!(overflow.is_err(), "超过上限时第三个应排队等待");

        // 释放一个后，另一个应立刻拿到。
        drop(a);
        let got = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            acquire_extraction(),
        )
        .await;
        assert!(got.is_ok(), "释放后应能拿到许可");

        drop(b);
        drop(got.ok().flatten());
    }

    /// Skill 闸门独立于抽取闸门（各自限流，互不占用）。
    #[tokio::test]
    async fn skill_gate_is_independent_from_extraction() {
        let _s = acquire_skill().await;
        assert!(_s.is_some());
        // 取 skill 许可不应影响抽取闸门的可用数
        let e = acquire_extraction().await;
        assert!(e.is_some(), "skill 闸门不应占用抽取名额");
    }
}
