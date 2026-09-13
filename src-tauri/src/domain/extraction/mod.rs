//! Extraction Run —— 异步抽取的后台任务实体（EXTRACTION-001）。
//!
//! 一次"分析文档"尝试被建模成一个 **Run**：它把"任务在不在跑"（status）
//! 与"跑到哪了"（stage）分开表达，二者组合才能让 UI 精确呈现，例如
//! `Running` + `Extracting`、`Failed` + `Comparing`。
//!
//! Run 必须持久化：用户关掉页面甚至关掉应用，任务状态都不该蒸发；
//! 重启后仍能看到 `Run #42` 是 `interrupted` 还是 `completed`。

use crate::string_enum;

string_enum! {
    /// Extraction Run 的生命周期状态。
    ///
    /// `status` 回答"任务在不在跑 / 结局如何"，`stage` 才回答"跑到哪了"。
    pub enum ExtractionRunStatus {
        /// 已创建、尚未开始（命令已返回 run_id，后台任务正要接手）。
        Queued => "queued",
        /// 正在执行。
        Running => "running",
        /// 成功跑完（结果在 `result_json`）。
        Completed => "completed",
        /// 执行中报错。
        Failed => "failed",
        /// 用户主动取消。
        Cancelled => "cancelled",
        /// 应用关闭时仍在跑，下次启动被标记为 interrupted（不假装还在跑）。
        Interrupted => "interrupted",
    }
}

impl ExtractionRunStatus {
    /// 终态：之后不再变化，后台任务也不应再改写它。
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            ExtractionRunStatus::Completed
                | ExtractionRunStatus::Failed
                | ExtractionRunStatus::Cancelled
                | ExtractionRunStatus::Interrupted
        )
    }
}

string_enum! {
    /// Extraction Run 的执行阶段（与 `status` 正交）。
    pub enum ExtractionStage {
        Preparing => "preparing",
        Chunking => "chunking",
        Extracting => "extracting",
        Validating => "validating",
        Comparing => "comparing",
        Finalizing => "finalizing",
    }
}

/// 一次抽取尝试的持久化记录。
///
/// 字段与 `extraction_runs` 表一一对应；`status` / `stage` 直接落库为规范字面量。
#[derive(Debug, Clone)]
pub struct ExtractionRun {
    pub id: String,
    pub document_id: String,
    pub status: ExtractionRunStatus,
    pub stage: ExtractionStage,
    pub total_chunks: i64,
    pub processed_chunks: i64,
    pub candidates_found: i64,
    pub changes_found: i64,
    /// 完成后的结果（序列化的 [`crate::application::dto::ExtractionReport`]），用于历史回看。
    pub result_json: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_roundtrip_through_their_literals() {
        assert_eq!(ExtractionRunStatus::Completed.as_str(), "completed");
        assert_eq!(
            "interrupted".parse::<ExtractionRunStatus>().unwrap(),
            ExtractionRunStatus::Interrupted
        );
    }

    #[test]
    fn terminal_statuses_are_exactly_the_four_endpoints() {
        assert!(ExtractionRunStatus::Completed.is_terminal());
        assert!(ExtractionRunStatus::Failed.is_terminal());
        assert!(ExtractionRunStatus::Cancelled.is_terminal());
        assert!(ExtractionRunStatus::Interrupted.is_terminal());
        assert!(!ExtractionRunStatus::Running.is_terminal());
        assert!(!ExtractionRunStatus::Queued.is_terminal());
    }

    #[test]
    fn stages_roundtrip_through_their_literals() {
        assert_eq!(ExtractionStage::Extracting.as_str(), "extracting");
        assert_eq!(
            "comparing".parse::<ExtractionStage>().unwrap(),
            ExtractionStage::Comparing
        );
    }
}
