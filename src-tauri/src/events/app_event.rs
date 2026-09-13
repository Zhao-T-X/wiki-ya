//! 应用事件。
//!
//! TDD 在 §53 与 §84 给了两套事件名，这里是它们的**并集**（分析报告 R7）：
//! 一套描述 Agent 运行时（`AgentStarted`…），一套描述知识变更
//! （`KnowledgeCommitted`…）。两者服务同一个目标——让"发生了什么"
//! 可以被 UI 流式呈现、被审计、被调试。
//!
//! 契约定性：**事件名一旦发布不得改名，只能新增**。
//! 前端与运行记录都会依赖这些字符串。

use serde::Serialize;

/// 应用事件。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppEvent {
    // ---- Agent 运行时（TDD §53）----
    AgentStarted {
        run_id: String,
        agent: String,
    },
    AgentThinking {
        run_id: String,
        text: String,
    },
    /// 流式文本增量（Agent 补全过程中逐段推送）。
    TokenDelta {
        run_id: String,
        delta: String,
    },
    ToolCalled {
        run_id: String,
        tool: String,
        arguments: serde_json::Value,
    },
    ToolCompleted {
        run_id: String,
        tool: String,
        ok: bool,
        summary: String,
    },
    AgentFinished {
        run_id: String,
        status: String,
    },

    // ---- 知识变更（TDD §84）----
    KnowledgeCommitted {
        document_id: String,
        claims: usize,
        relations: usize,
    },
    ClaimEvolutionCreated {
        relation_id: String,
        relationship: String,
        status: String,
    },
    ConflictDetected {
        claim_id: String,
        related_claim_id: String,
    },
    ReviewRequired {
        review_id: String,
        target: String,
    },
    SearchCompleted {
        query: String,
        hits: usize,
        took_ms: u64,
        method: String,
    },
    EmbeddingUpdated {
        chunks: usize,
    },
}

impl AppEvent {
    /// 稳定的事件名（用于审计日志与前端订阅）。
    pub fn name(&self) -> &'static str {
        match self {
            AppEvent::AgentStarted { .. } => "agent_started",
            AppEvent::AgentThinking { .. } => "agent_thinking",
            AppEvent::TokenDelta { .. } => "token_delta",
            AppEvent::ToolCalled { .. } => "tool_called",
            AppEvent::ToolCompleted { .. } => "tool_completed",
            AppEvent::AgentFinished { .. } => "agent_finished",
            AppEvent::KnowledgeCommitted { .. } => "knowledge_committed",
            AppEvent::ClaimEvolutionCreated { .. } => "claim_evolution_created",
            AppEvent::ConflictDetected { .. } => "conflict_detected",
            AppEvent::ReviewRequired { .. } => "review_required",
            AppEvent::SearchCompleted { .. } => "search_completed",
            AppEvent::EmbeddingUpdated { .. } => "embedding_updated",
        }
    }

    /// 该事件是否代表用户需要介入。
    ///
    /// 只有它需要打断用户——其余事件只是状态播报。
    pub fn needs_attention(&self) -> bool {
        matches!(
            self,
            AppEvent::ReviewRequired { .. } | AppEvent::ConflictDetected { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_serialise_with_a_stable_type_tag() {
        let event = AppEvent::KnowledgeCommitted {
            document_id: "d1".into(),
            claims: 3,
            relations: 1,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "knowledge_committed");
        assert_eq!(json["claims"], 3);
    }

    #[test]
    fn only_review_and_conflict_events_need_attention() {
        assert!(AppEvent::ConflictDetected {
            claim_id: "c1".into(),
            related_claim_id: "c2".into(),
        }
        .needs_attention());
        assert!(!AppEvent::EmbeddingUpdated { chunks: 1 }.needs_attention());
    }

    #[test]
    fn every_variant_has_a_unique_name() {
        let names = [
            AppEvent::AgentStarted {
                run_id: String::new(),
                agent: String::new(),
            }
            .name(),
            AppEvent::AgentThinking {
                run_id: String::new(),
                text: String::new(),
            }
            .name(),
            AppEvent::TokenDelta {
                run_id: String::new(),
                delta: String::new(),
            }
            .name(),
            AppEvent::ToolCalled {
                run_id: String::new(),
                tool: String::new(),
                arguments: serde_json::json!({}),
            }
            .name(),
            AppEvent::ToolCompleted {
                run_id: String::new(),
                tool: String::new(),
                ok: true,
                summary: String::new(),
            }
            .name(),
            AppEvent::AgentFinished {
                run_id: String::new(),
                status: String::new(),
            }
            .name(),
            AppEvent::KnowledgeCommitted {
                document_id: String::new(),
                claims: 0,
                relations: 0,
            }
            .name(),
            AppEvent::ClaimEvolutionCreated {
                relation_id: String::new(),
                relationship: String::new(),
                status: String::new(),
            }
            .name(),
            AppEvent::ConflictDetected {
                claim_id: String::new(),
                related_claim_id: String::new(),
            }
            .name(),
            AppEvent::ReviewRequired {
                review_id: String::new(),
                target: String::new(),
            }
            .name(),
            AppEvent::SearchCompleted {
                query: String::new(),
                hits: 0,
                took_ms: 0,
                method: String::new(),
            }
            .name(),
            AppEvent::EmbeddingUpdated { chunks: 0 }.name(),
        ];
        let unique: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len());
    }
}
