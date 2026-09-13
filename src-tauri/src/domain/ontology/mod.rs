//! Ontology —— wiki-ya 的 Domain Core（Rule 5）。
//!
//! 它回答三个问题：
//! 1. 世界上有哪些**稳定对象**（[`entity`]）以及它们被允许的类型（[`entity_type`]）
//! 2. 对象之间可以建立哪些**受控谓语**（[`predicate`]、[`relation`]）
//! 3. 一段自由文本怎样被归一化并落到受控词表上（[`normalization`]、[`resolution`]）
//!
//! 注册表（[`registry`]）是这一切的版本化事实来源。

pub mod entity;
pub mod entity_type;
pub mod event;
pub mod normalization;
pub mod predicate;
pub mod registry;
pub mod relation;
pub mod resolution;
