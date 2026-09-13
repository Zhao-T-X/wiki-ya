//! Event —— 发生过的事情。
//!
//! Event 与 Claim 的区别：Claim 断言"是什么"，Event 断言"何时发生了什么"。
//! 时间在这里是一等公民，因此 `EventTime` 带 `precision`——
//! 只知道"2024 年发布"和精确到秒的"2024-03-15 09:00:00"是两种不同的知识，
//! 用同一个 `DateTime` 表示它们会丢掉这个区别。

use crate::domain::common::ids::EventId;
use crate::domain::common::Timestamp;
use crate::string_enum;

string_enum! {
    /// 事件类型（18 个）。
    pub enum EventType {
        Creation => "creation",
        Development => "development",
        Release => "release",
        Publication => "publication",
        Deployment => "deployment",
        Acquisition => "acquisition",
        Merger => "merger",
        Migration => "migration",
        Training => "training",
        Evaluation => "evaluation",
        Experiment => "experiment",
        Update => "update",
        Decision => "decision",
        Announcement => "announcement",
        Meeting => "meeting",
        Failure => "failure",
        Incident => "incident",
        Other => "other",
    }
}

impl EventType {
    pub const DEFAULT: EventType = EventType::Other;
}

string_enum! {
    /// 事件状态（6 个）。
    pub enum EventStatus {
        Planned => "planned",
        Ongoing => "ongoing",
        Completed => "completed",
        Cancelled => "cancelled",
        Failed => "failed",
        Unknown => "unknown",
    }
}

impl EventStatus {
    pub const DEFAULT: EventStatus = EventStatus::Unknown;
}

string_enum! {
    /// 时间精度（7 级）。
    ///
    /// 这是「Temporal Knowledge」（PRD §20）能成立的细节基础：
    /// 精度决定了一个时间点能覆盖多大的区间。
    pub enum EventTimePrecision {
        Exact => "exact",
        Day => "day",
        Month => "month",
        Year => "year",
        Range => "range",
        Relative => "relative",
        Unknown => "unknown",
    }
}

/// 事件时间。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventTime {
    pub start: Option<Timestamp>,
    pub end: Option<Timestamp>,
    pub precision: EventTimePrecision,
}

impl Default for EventTime {
    fn default() -> Self {
        EventTime {
            start: None,
            end: None,
            precision: EventTimePrecision::Unknown,
        }
    }
}

impl EventTime {
    /// 是否有任何可用的时间信息。
    pub fn is_known(&self) -> bool {
        self.start.is_some() || self.end.is_some()
    }
}

/// 事件。
#[derive(Debug, Clone)]
pub struct Event {
    pub id: EventId,
    pub event_type: EventType,
    pub description: String,
    pub participants: Vec<String>,
    pub time: EventTime,
    pub location: Option<String>,
    pub status: EventStatus,
    pub confidence: Option<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_sizes_are_frozen() {
        assert_eq!(EventType::ALL.len(), 18);
        assert_eq!(EventStatus::ALL.len(), 6);
        assert_eq!(EventTimePrecision::ALL.len(), 7);
    }

    #[test]
    fn defaults_match_the_reference_implementation() {
        assert_eq!(EventType::DEFAULT, EventType::Other);
        assert_eq!(EventStatus::DEFAULT, EventStatus::Unknown);
        let time = EventTime::default();
        assert!(!time.is_known());
        assert_eq!(time.precision, EventTimePrecision::Unknown);
    }

    #[test]
    fn partial_dates_keep_their_precision() {
        let time = EventTime {
            start: Some("2024".into()),
            end: None,
            precision: EventTimePrecision::Year,
        };
        assert!(time.is_known());
        assert_ne!(time.precision, EventTimePrecision::Exact);
    }
}
