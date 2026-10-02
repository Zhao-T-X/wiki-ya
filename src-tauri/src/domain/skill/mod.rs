//! Skill —— Agent 的可复用能力单元（M2）。
//!
//! Agent 不再定义"我会干什么"，而是声明"我拥有哪些 Skill"（M4）。
//! Skill 是最小的可执行能力单元：有名字、有版本、声明权限、吃结构化
//! 输入、产出候选/答案/提案——**永不直接修改知识**（PROPOSE ≠ MUTATE）。
//!
//! 三条边界（行动计划纪律三）：
//! - Skill 可以自由组合，Ontology 不可以失控；
//! - Skill 产出一律走 Candidate / Proposal → Review；
//! - Skill 的权限声明写入 Run metadata，Trace 可回答"谁能做什么"。

use crate::string_enum;

string_enum! {
    /// 内置 Skill 名（M2 第一批：只做三个）。
    pub enum SkillName {
        /// 从文档抽取候选知识（复用抽取管线）。
        KnowledgeExtraction => "knowledge-extraction",
        /// 基于知识库回答问题（复用 Ask 编排）。
        KnowledgeAnswering => "knowledge-answering",
        /// 对比已有知识、发现冲突并产生纠正提案（复用演化分析）。
        KnowledgeCorrection => "knowledge-correction",
    }
}

string_enum! {
    /// Skill 权限（M5 会扩展为完整的 Policy 闸门；M2 只声明 + 留痕）。
    pub enum SkillPermission {
        /// 只读知识库。
        Read => "read",
        /// 可以产出候选 / 提案（进入 Review 队列）。
        Propose => "propose",
    }
}

/// Skill 的静态描述（注册表条目，供 Agent Profile 与前端枚举）。
#[derive(Debug, Clone)]
pub struct SkillDescriptor {
    pub name: SkillName,
    pub description: &'static str,
    pub permissions: &'static [SkillPermission],
    /// 输入字段说明（M3 标准化时升级为结构化 schema）。
    pub input_hint: &'static str,
}

impl SkillDescriptor {
    pub fn has_permission(&self, permission: SkillPermission) -> bool {
        self.permissions.contains(&permission)
    }
}

/// 从 `skill_versions` 解析出的完整 Skill 定义（版本化）。
#[derive(Debug, Clone)]
pub struct SkillDefinition {
    pub name: SkillName,
    pub version: i64,
    pub description: String,
    pub instructions: String,
    pub input_hint: String,
    pub output_hint: String,
    pub tools: Vec<String>,
    pub permissions: Vec<SkillPermission>,
}

impl SkillDefinition {
    /// `knowledge-extraction@1` 形式的稳定标识，写进 Run 的 actor。
    pub fn qualified_name(&self) -> String {
        format!("{}@{}", self.name.as_str(), self.version)
    }

    pub fn has_permission(&self, permission: SkillPermission) -> bool {
        self.permissions.contains(&permission)
    }
}

/// 内置 Skill 注册表（M11 之前是静态的；之后迁移为可自定义）。
pub fn registry() -> Vec<SkillDescriptor> {
    vec![
        SkillDescriptor {
            name: SkillName::KnowledgeExtraction,
            description: "从一篇文档中抽取结构化的候选知识（只预览，不落库）",
            permissions: &[SkillPermission::Read, SkillPermission::Propose],
            input_hint: r#"{ "documentId": "<文档 id>" }"#,
        },
        SkillDescriptor {
            name: SkillName::KnowledgeAnswering,
            description: "基于知识库回答问题，回答带来源引用",
            permissions: &[SkillPermission::Read],
            input_hint: r#"{ "question": "问题", "role": "auto|knowledge|research|..." }"#,
        },
        SkillDescriptor {
            name: SkillName::KnowledgeCorrection,
            description: "对比文档内容与已有知识，把冲突/新增提案送入 Review",
            permissions: &[SkillPermission::Read, SkillPermission::Propose],
            input_hint: r#"{ "documentId": "<文档 id>" }"#,
        },
    ]
}
