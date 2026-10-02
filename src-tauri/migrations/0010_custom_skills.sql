-- 0010 Skill 自定义（M11）。
--
-- 边界：自定义 Skill 强制只读（permissions 恒为 ["read"]）——
-- 用户可自定义「怎么处理文本」，产不出候选/提案，Ontology 与
-- Knowledge Policy 不可能被绕过。PROPOSE 型自定义 Skill 需要结构化
-- 输出 schema 校验，留待后续版本。
ALTER TABLE skills ADD COLUMN is_builtin INTEGER NOT NULL DEFAULT 0;
