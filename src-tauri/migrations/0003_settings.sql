-- ============================================================================
-- wiki-ya 简单键值设置（0003，AI 配置化）
--
-- 与 0001 / 0002 同机执行：纯 DDL，带 IF NOT EXISTS 保证幂等。
--
-- 用途：把 AI 运行时配置（API Key / Base URL / Chat 模型 / 向量模型）从
-- 「只能环境变量预设」升级为「应用内设置页可持久化、保存即生效」。
-- 优先级由读取端（ai::config::AiConfig::from_settings）保证：
--   持久化设置 > 环境变量 > 默认值。
-- ============================================================================

CREATE TABLE IF NOT EXISTS settings (
  key       TEXT PRIMARY KEY,
  value     TEXT NOT NULL,
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
