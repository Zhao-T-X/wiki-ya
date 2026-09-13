//! Entity 与别名的持久化，以及 Entity Resolution 需要的查表操作。

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::common::ids::EntityId;
use crate::domain::ontology::entity::{Entity, EntityStatus};
use crate::domain::ontology::entity_type::EntityType;
use crate::domain::ontology::resolution::normalize_name;
use crate::error::{AppError, AppResult};
use crate::infrastructure::db::{json_col, parse_col, string_list_col};

/// 实体 + 计数（UI 卡片需要）。
#[derive(Debug, Clone)]
pub struct EntityRow {
    pub entity: Entity,
    pub alias_count: i64,
    pub claim_count: i64,
}

const ENTITY_COLUMNS: &str =
    "id, name, primary_type, types_json, description, properties_json, status, created_at, updated_at";

fn map_entity(row: &rusqlite::Row<'_>) -> rusqlite::Result<Entity> {
    let types: Vec<String> = string_list_col(row, 3)?;
    let types = types
        .iter()
        .map(|raw| {
            raw.parse::<EntityType>().map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    3,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })
        })
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(Entity {
        id: parse_col::<EntityId>(row, 0)?,
        name: row.get(1)?,
        primary_type: parse_col::<EntityType>(row, 2)?,
        types,
        description: row.get(4)?,
        properties: json_col(row, 5)?,
        status: parse_col::<EntityStatus>(row, 6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

/// 插入实体。同名（大小写不敏感）会返回 `Conflict`（INV-03）。
pub fn insert(conn: &Connection, entity: &Entity) -> AppResult<()> {
    let types: Vec<&str> = entity.types.iter().map(|t| t.as_str()).collect();
    conn.execute(
        "INSERT INTO entities(id,name,primary_type,types_json,description,properties_json,status)
         VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            entity.id.as_str(),
            entity.name,
            entity.primary_type.as_str(),
            serde_json::to_string(&types)?,
            entity.description,
            serde_json::to_string(&entity.properties)?,
            entity.status.as_str(),
        ],
    )?;
    Ok(())
}

/// 按 id 取实体。
pub fn find_by_id(conn: &Connection, id: &EntityId) -> AppResult<Option<Entity>> {
    let sql = format!("SELECT {ENTITY_COLUMNS} FROM entities WHERE id = ?1");
    Ok(conn
        .query_row(&sql, params![id.as_str()], map_entity)
        .optional()?)
}

/// 按规范名精确匹配（大小写不敏感，走 `lower(name)` 唯一索引 → Resolution 的 Exact/Normalized 级）。
pub fn find_by_name(conn: &Connection, name: &str) -> AppResult<Option<Entity>> {
    let sql = format!("SELECT {ENTITY_COLUMNS} FROM entities WHERE lower(name) = lower(?1)");
    Ok(conn
        .query_row(&sql, params![name.trim()], map_entity)
        .optional()?)
}

/// 按别名匹配（Resolution 的 Alias 级）。
pub fn find_by_alias(conn: &Connection, alias: &str) -> AppResult<Option<Entity>> {
    let normalized = normalize_name(alias);
    if normalized.is_empty() {
        return Ok(None);
    }
    let sql = format!(
        "SELECT {}
         FROM entities e
         JOIN entity_aliases a ON a.entity_id = e.id
         WHERE a.alias_normalized = ?1
         LIMIT 1",
        ENTITY_COLUMNS
            .split(", ")
            .map(|column| format!("e.{column}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(conn
        .query_row(&sql, params![normalized], map_entity)
        .optional()?)
}

/// 实体 + 计数。
pub fn get_row(conn: &Connection, id: &EntityId) -> AppResult<Option<EntityRow>> {
    let sql = format!(
        "SELECT {},
                (SELECT COUNT(*) FROM entity_aliases a WHERE a.entity_id = e.id),
                (SELECT COUNT(*) FROM claims c WHERE c.subject_id = e.id)
         FROM entities e WHERE e.id = ?1",
        ENTITY_COLUMNS
            .split(", ")
            .map(|column| format!("e.{column}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(conn
        .query_row(&sql, params![id.as_str()], |row| {
            Ok(EntityRow {
                entity: map_entity(row)?,
                alias_count: row.get(9)?,
                claim_count: row.get(10)?,
            })
        })
        .optional()?)
}

/// 列出实体（可按关键词与类型过滤），claim 数多的排前面。
pub fn list(
    conn: &Connection,
    query: Option<&str>,
    entity_type: Option<EntityType>,
    limit: usize,
) -> AppResult<Vec<EntityRow>> {
    let limit = limit.clamp(1, 500) as i64;
    let pattern = query
        .map(|q| q.trim())
        .filter(|q| !q.is_empty())
        .map(|q| format!("%{q}%"));

    let sql = format!(
        "SELECT {},
                (SELECT COUNT(*) FROM entity_aliases a WHERE a.entity_id = e.id),
                (SELECT COUNT(*) FROM claims c WHERE c.subject_id = e.id)
         FROM entities e
         WHERE (?1 IS NULL OR e.name LIKE ?1)
           AND (?2 IS NULL OR e.primary_type = ?2)
           AND e.status NOT IN ('rejected','archived')
         ORDER BY (SELECT COUNT(*) FROM claims c WHERE c.subject_id = e.id) DESC, e.name
         LIMIT ?3",
        ENTITY_COLUMNS
            .split(", ")
            .map(|column| format!("e.{column}"))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(
        params![pattern, entity_type.map(|t| t.as_str()), limit],
        |row| {
            Ok(EntityRow {
                entity: map_entity(row)?,
                alias_count: row.get(9)?,
                claim_count: row.get(10)?,
            })
        },
    )?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 写入别名。重复别名静默忽略（同一实体内别名唯一，INV-04）。
pub fn insert_alias(conn: &Connection, entity_id: &EntityId, alias: &str) -> AppResult<()> {
    let normalized = normalize_name(alias);
    if normalized.is_empty() {
        return Err(AppError::Invalid("别名不能为空".into()));
    }
    conn.execute(
        "INSERT OR IGNORE INTO entity_aliases(entity_id, alias, alias_normalized)
         VALUES(?1,?2,?3)",
        params![entity_id.as_str(), alias.trim(), normalized],
    )?;
    Ok(())
}

/// 列出别名。
pub fn list_aliases(conn: &Connection, entity_id: &EntityId) -> AppResult<Vec<String>> {
    let mut statement = conn.prepare(
        "SELECT alias FROM entity_aliases WHERE entity_id = ?1 ORDER BY alias",
    )?;
    let rows = statement.query_map(params![entity_id.as_str()], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 按名称解析实体，不存在则创建。
///
/// 这是**手动录入 Claim** 的落地路径（AI 关闭时的降级方案）：
/// 用户写一个名字，系统尽量挂到已有实体上，否则建一个新的候选实体。
///
/// 消解顺序遵循 `resolution` 模块的约定：Exact/Normalized → Alias → 新建。
/// Fuzzy/Semantic 不在此处自动合并——它们必须走人工审核。
pub fn resolve_or_create(conn: &Connection, name: &str) -> AppResult<Entity> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AppError::Invalid("实体名称不能为空".into()));
    }

    if let Some(found) = find_by_name(conn, trimmed)? {
        return Ok(found);
    }
    if let Some(found) = find_by_alias(conn, trimmed)? {
        return Ok(found);
    }

    let entity = Entity::new(trimmed, vec![EntityType::DEFAULT])?;
    match insert(conn, &entity) {
        Ok(()) => Ok(entity),
        // 并发下可能刚被别的调用插入：此时回到查询，而不是报错失败。
        Err(AppError::Conflict(_)) => find_by_name(conn, trimmed)?
            .ok_or_else(|| AppError::Internal("实体刚创建却查不到，数据可能不一致".into())),
        Err(other) => Err(other),
    }
}

/// 实体总数。
pub fn count(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM entities", [], |row| row.get(0))?)
}

/// 尚未归类的实体数。
///
/// 定义：仍处于 `candidate` 且主类型是兜底的 `Resource`。
/// 这类实体通常意味着"系统见到了一个东西，但不知道它是什么"，
/// 正是 Knowledge Health 该提醒用户处理的对象。
pub fn count_unresolved(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM entities
         WHERE status = 'candidate' AND primary_type = 'Resource'",
        [],
        |row| row.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::db::tests::memory_db;

    fn entity(name: &str, types: Vec<EntityType>) -> Entity {
        Entity::new(name, types).unwrap()
    }

    #[test]
    fn insert_and_read_round_trip_types() {
        let conn = memory_db();
        let stored = entity(
            "SQLite",
            vec![EntityType::Software, EntityType::Product],
        );
        insert(&conn, &stored).unwrap();

        let loaded = find_by_id(&conn, &stored.id).unwrap().unwrap();
        assert_eq!(loaded.name, "SQLite");
        assert_eq!(loaded.primary_type, EntityType::Software);
        assert_eq!(loaded.types.len(), 2);
        assert_eq!(loaded.status, EntityStatus::Candidate);
    }

    #[test]
    fn name_lookup_is_case_insensitive() {
        let conn = memory_db();
        insert(&conn, &entity("OpenAI", vec![EntityType::Organization])).unwrap();
        assert!(find_by_name(&conn, "openai").unwrap().is_some());
        assert!(find_by_name(&conn, "OPENAI").unwrap().is_some());
        assert!(find_by_name(&conn, "  OpenAI  ").unwrap().is_some());
    }

    #[test]
    fn alias_lookup_resolves_to_the_owning_entity() {
        let conn = memory_db();
        let stored = entity("OpenAI", vec![EntityType::Organization]);
        insert(&conn, &stored).unwrap();
        insert_alias(&conn, &stored.id, "OpenAI Inc.").unwrap();
        insert_alias(&conn, &stored.id, "openai inc.").unwrap(); // 归一后与上一条相同 → 忽略

        let aliases = list_aliases(&conn, &stored.id).unwrap();
        assert_eq!(aliases.len(), 1);

        let found = find_by_alias(&conn, "  OPENAI   INC. ").unwrap().unwrap();
        assert_eq!(found.id, stored.id);
    }

    #[test]
    fn resolve_or_create_prefers_existing_entities_then_creates_a_candidate() {
        let conn = memory_db();
        let existing = entity("Rust", vec![EntityType::Technology]);
        insert(&conn, &existing).unwrap();

        let resolved = resolve_or_create(&conn, "rust").unwrap();
        assert_eq!(resolved.id, existing.id);

        let created = resolve_or_create(&conn, "Tauri").unwrap();
        assert_eq!(created.name, "Tauri");
        assert_eq!(created.status, EntityStatus::Candidate);
        assert_eq!(created.primary_type, EntityType::Resource);

        // 第二次解析同一个新名字必须复用，而不是再建一个
        let again = resolve_or_create(&conn, "Tauri").unwrap();
        assert_eq!(again.id, created.id);
        assert_eq!(count(&conn).unwrap(), 2);
    }

    #[test]
    fn resolve_or_create_rejects_blank_names() {
        let conn = memory_db();
        assert!(resolve_or_create(&conn, "   ").is_err());
    }

    #[test]
    fn list_filters_and_sorts_by_claim_count() {
        let conn = memory_db();
        let rust = entity("Rust", vec![EntityType::Technology]);
        let tauri = entity("Tauri", vec![EntityType::Software]);
        insert(&conn, &rust).unwrap();
        insert(&conn, &tauri).unwrap();

        conn.execute(
            "INSERT INTO claims(id,subject_id,predicate) VALUES('c1',?1,'uses')",
            params![rust.id.as_str()],
        )
        .unwrap();

        let rows = list(&conn, None, None, 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].entity.name, "Rust"); // claim 多者在前
        assert_eq!(rows[0].claim_count, 1);

        let filtered = list(&conn, None, Some(EntityType::Software), 10).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].entity.name, "Tauri");

        let searched = list(&conn, Some("tau"), None, 10).unwrap();
        assert_eq!(searched.len(), 1);
    }

    #[test]
    fn unresolved_entities_are_the_fallback_typed_candidates() {
        let conn = memory_db();
        insert(&conn, &entity("Tauri", vec![EntityType::Software])).unwrap();
        assert_eq!(count_unresolved(&conn).unwrap(), 0);

        resolve_or_create(&conn, "Something Opaque").unwrap();
        assert_eq!(count_unresolved(&conn).unwrap(), 1);
    }

    #[test]
    fn corrupted_types_json_is_reported() {
        let conn = memory_db();
        conn.execute(
            "INSERT INTO entities(id,name,primary_type,types_json) VALUES('e1','X','Concept','[\"NotAType\"]')",
            [],
        )
        .unwrap();
        assert!(find_by_id(&conn, &EntityId::from_raw("e1")).is_err());
    }
}
