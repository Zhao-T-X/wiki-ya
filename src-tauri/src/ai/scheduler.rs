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

    #[tokio::test]
    async fn extraction_gate_limits_concurrency() {
        // 连取 3 个许可，第 3 个必须拿不到（上限 2）。
        let a = acquire_extraction().await;
        let b = acquire_extraction().await;
        assert!(a.is_some() && b.is_some(), "前两个应拿到许可");
        assert_eq!(extraction_in_flight().await, EXTRACTION_CAPACITY);

        // 第三个不应立刻拿到——用 timeout 证明它在排队而不是被拒。
        let third = tokio::time::timeout(std::time::Duration::from_millis(50), acquire_extraction()).await;
        assert!(third.is_err(), "超过上限时第三个应处于排队等待，而不是立即失败");

        drop(a);
        drop(b);
    }

    #[tokio::test]
    async fn released_permit_becomes_available() {
        let first = acquire_extraction().await;
        assert!(first.is_some());
        let waited = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            acquire_extraction(),
        )
        .await;
        assert!(waited.is_ok(), "释放后应能拿到许可");
    }
}
