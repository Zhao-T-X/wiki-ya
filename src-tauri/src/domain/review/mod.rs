//! Review —— 所有 AI 提案与知识变更的统一审核入口。
//!
//! PRD §42 要求每个 Review 都必须回答四个问题：
//! **What changed? Why? Evidence? Impact?**
//!
//! 这不是文案要求，而是产品原则「AI suggests, user decides」的落地点：
//! 用户无法审核一个说不清自己在改什么的提案。因此
//! [`review::describe`] 必须为四问中的「变化」与「影响」各自产出确定性文本，
//! 具体的卡片组装（含 [`crate::application::dto::ReviewItem`]）交给应用层。

pub mod review;
pub mod rules;
