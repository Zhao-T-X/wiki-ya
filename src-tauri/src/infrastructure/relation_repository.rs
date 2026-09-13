//! 实体 → 实体关系的持久化（`relations` 表）。
//!
//! 与 `claim_relations` 的区别是本质性的：这张表存**结构性事实**
//! （`wiki-ya uses SQLite`），那张表存**知识演化的历史**。
//! 前者没有 `superseded`（决策 D6），后者有。

use rusqlite::{params, Connection};

use crate::domain::common::ids::{EntityId, RelationId};
use crate::domain::ontology::predicate::RelationPredicate;
use crate::domain::ontology::relation::{Relation, RelationStatus};
use crate::error::AppResult;
use crate::infrastructure::db::{opt_f32_col, parse_col};

/// 关系 + 两端实体名。
#[derive(Debug, Clone)]
pub struct RelationRow {
    pub id: RelationId,
    pub source_id: EntityId,
    pub source_name: String,
    pub predicate: RelationPredicate,
    pub target_id: EntityId,
    pub target_name: String,
    pub confidence: Option<f32>,
    pub status: RelationStatus,
}

const RELATION_SELECT: &str = "
SELECT r.id, r.source_id, s.name, r.predicate, r.target_id, t.name, r.confidence, r.status
FROM relations r
JOIN entities s ON s.id = r.source_id
JOIN entities t ON t.id = r.target_id
";

fn map_relation(row: &rusqlite::Row<'_>) -> rusqlite::Result<RelationRow> {
    Ok(RelationRow {
        id: parse_col::<RelationId>(row, 0)?,
        source_id: parse_col::<EntityId>(row, 1)?,
        source_name: row.get(2)?,
        predicate: parse_col::<RelationPredicate>(row, 3)?,
        target_id: parse_col::<EntityId>(row, 4)?,
        target_name: row.get(5)?,
        confidence: opt_f32_col(row, 6)?,
        status: parse_col::<RelationStatus>(row, 7)?,
    })
}

/// 插入关系；同一个 (source, predicate, target) 已存在时忽略。
pub fn insert(
    conn: &Connection,
    source_id: &EntityId,
    predicate: RelationPredicate,
    target_id: &EntityId,
    confidence: Option<f32>,
    evidence_document_id: Option<&str>,
    evidence_chunk_id: Option<&str>,
    evidence_quote: Option<&str>,
) -> AppResult<bool> {
    let id = RelationId::new();
    let affected = conn.execute(
        "INSERT OR IGNORE INTO relations(
            id, source_id, predicate, target_id, confidence,
            evidence_document_id, evidence_chunk_id, evidence_quote, created_by
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'system')",
        params![
            id.as_str(),
            source_id.as_str(),
            predicate.as_str(),
            target_id.as_str(),
            confidence.map(f64::from),
            evidence_document_id,
            evidence_chunk_id,
            evidence_quote,
        ],
    )?;
    Ok(affected > 0)
}

/// 与某个实体相关的全部关系（双向）。
pub fn list_for_entity(conn: &Connection, entity_id: &EntityId) -> AppResult<Vec<RelationRow>> {
    let sql = format!(
        "{RELATION_SELECT}
         WHERE r.source_id = ?1 OR r.target_id = ?1
         ORDER BY r.created_at"
    );
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params![entity_id.as_str()], map_relation)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 所有关系（供图探索与图谱页使用）。
///
/// 上限是刻意的：图谱只服务"某个实体的邻域"，从来不需要全量边。
pub fn list_all(conn: &Connection, limit: usize) -> AppResult<Vec<RelationRow>> {
    let sql = format!("{RELATION_SELECT} ORDER BY r.created_at LIMIT ?1");
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params![limit.clamp(1, 5_000) as i64], map_relation)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 关系总数。
pub fn count(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM relations", [], |row| row.get(0))?)
}

/// 把领域对象写入（供未来的抽取路径使用）。
pub fn insert_domain(conn: &Connection, relation: &Relation) -> AppResult<bool> {
    insert(
        conn,
        &relation.source_id,
        relation.predicate,
        &relation.target_id,
        relation.confidence,
        None,
        None,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::db::tests::memory_db;
    use crate::infrastructure::entity_repository;

    fn two_entities(conn: &Connection) -> (EntityId, EntityId) {
        let rust = entity_repository::resolve_or_create(conn, "wiki-ya").unwrap();
        let sqlite = entity_repository::resolve_or_create(conn, "SQLite").unwrap();
        // resolve_or_create 建出来的是 Resource；develops 之类需要具体类型，
        // 这里用 uses（通配两端），因此无需改类型。
        (rust.id, sqlite.id)
    }

    #[test]
    fn insert_is_idempotent_for_the_same_triple() {
        let conn = memory_db();
        let (source, target) = two_entities(&conn);
        assert!(insert(&conn, &source, RelationPredicate::Uses, &target, None, None, None, None)
            .unwrap());
        assert!(!insert(&conn, &source, RelationPredicate::Uses, &target, None, None, None, None)
            .unwrap());
        assert_eq!(count(&conn).unwrap(), 1);
    }

    #[test]
    fn list_for_entity_returns_both_directions_with_names() {
        let conn = memory_db();
        let (source, target) = two_entities(&conn);
        insert(&conn, &source, RelationPredicate::Uses, &target, Some(0.9), None, None, None)
            .unwrap();

        let from_source = list_for_entity(&conn, &source).unwrap();
        assert_eq!(from_source.len(), 1);
        assert_eq!(from_source[0].source_name, "wiki-ya");
        assert_eq!(from_source[0].target_name, "SQLite");
        assert_eq!(from_source[0].predicate, RelationPredicate::Uses);
        assert_eq!(from_source[0].confidence, Some(0.9));

        let from_target = list_for_entity(&conn, &target).unwrap();
        assert_eq!(from_target.len(), 1, "反向也必须能查到同一条关系");
    }

    #[test]
    fn relation_status_never_contains_superseded() {
        let conn = memory_db();
        let (source, target) = two_entities(&conn);
        insert(&conn, &source, RelationPredicate::Uses, &target, None, None, None, None).unwrap();
        let result = conn.execute("UPDATE relations SET status = 'superseded'", []);
        assert!(result.is_err(), "实体关系的生命周期里没有 superseded（决策 D6）");
    }

    #[test]
    fn unknown_predicate_in_the_database_is_reported_on_read() {
        let conn = memory_db();
        let (source, target) = two_entities(&conn);
        conn.execute(
            "INSERT INTO relations(id,source_id,predicate,target_id) VALUES('r1',?1,'vibes_with',?2)",
            params![source.as_str(), target.as_str()],
        )
        .unwrap();
        assert!(list_for_entity(&conn, &source).is_err());
    }

    #[test]
    fn deleting_an_entity_cascades_is_blocked_by_foreign_keys() {
        let conn = memory_db();
        let (source, target) = two_entities(&conn);
        insert(&conn, &source, RelationPredicate::Uses, &target, None, None, None, None).unwrap();
        // relations 没有声明 ON DELETE，因此删除被引用的实体会被外键阻止：
        // 这是有意的——关系是知识，不该因为删一个实体而静默消失。
        let result = conn.execute("DELETE FROM entities WHERE id = ?1", params![target.as_str()]);
        assert!(result.is_err());
    }
}
