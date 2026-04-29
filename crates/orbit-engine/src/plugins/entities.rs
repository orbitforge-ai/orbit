//! Plugin-defined entity storage. A single generic `plugin_entities` table
//! backs every plugin's entity types; the manifest JSON Schema is the
//! typing/validation layer. Relations live in `plugin_entity_relations`.
//!
//! The unix-socket core-API server (`ORBIT_CORE_API_SOCKET`) that plugins
//! call back into is implemented in a follow-up slice; the DB access path
//! below is the primitive both `EntityToolHandler` and that future socket
//! server will share.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ulid::Ulid;

use crate::db::DbPool;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginEntity {
    pub id: String,
    pub plugin_id: String,
    pub entity_type: String,
    pub project_id: Option<String>,
    pub data: Value,
    pub created_by_agent_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginEntityRelation {
    pub id: String,
    pub from_kind: String,
    pub from_type: String,
    pub from_id: String,
    pub to_kind: String,
    pub to_type: String,
    pub to_id: String,
    pub relation: String,
    pub created_at: String,
}

/// Filter passed to `list`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListFilter {
    pub project_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub fn create(
    db: &DbPool,
    plugin_id: &str,
    entity_type: &str,
    project_id: Option<&str>,
    data: &Value,
    created_by_agent_id: Option<&str>,
) -> Result<PluginEntity, String> {
    let conn = db.get().map_err(|e| e.to_string())?;
    let id = Ulid::new().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let data_str = serde_json::to_string(data)
        .map_err(|e| format!("failed to serialize entity data: {}", e))?;

    conn.execute(
        "INSERT INTO plugin_entities (id, plugin_id, entity_type, project_id, data,
                                       created_by_agent_id, created_at, updated_at, tenant_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, COALESCE((SELECT tenant_id FROM projects WHERE id = ?4), (SELECT tenant_id FROM agents WHERE id = ?6), 'local'))",
        rusqlite::params![
            id,
            plugin_id,
            entity_type,
            project_id,
            data_str,
            created_by_agent_id,
            now,
        ],
    )
    .map_err(|e| format!("insert plugin_entity: {}", e))?;

    Ok(PluginEntity {
        id,
        plugin_id: plugin_id.to_string(),
        entity_type: entity_type.to_string(),
        project_id: project_id.map(str::to_string),
        data: data.clone(),
        created_by_agent_id: created_by_agent_id.map(str::to_string),
        created_at: now.clone(),
        updated_at: now,
    })
}

pub fn get(db: &DbPool, id: &str) -> Result<Option<PluginEntity>, String> {
    let conn = db.get().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, plugin_id, entity_type, project_id, data,
                    created_by_agent_id, created_at, updated_at
             FROM plugin_entities WHERE id = ?1",
        )
        .map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query_map(rusqlite::params![id], row_to_entity)
        .map_err(|e| e.to_string())?;
    Ok(rows.next().transpose().map_err(|e| e.to_string())?)
}

pub fn list(
    db: &DbPool,
    plugin_id: &str,
    entity_type: &str,
    filter: &ListFilter,
) -> Result<Vec<PluginEntity>, String> {
    let conn = db.get().map_err(|e| e.to_string())?;
    let limit = filter.limit.unwrap_or(100).max(1).min(500);
    let offset = filter.offset.unwrap_or(0).max(0);
    let mut sql = String::from(
        "SELECT id, plugin_id, entity_type, project_id, data,
                created_by_agent_id, created_at, updated_at
         FROM plugin_entities
         WHERE plugin_id = ?1 AND entity_type = ?2",
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![
        Box::new(plugin_id.to_string()),
        Box::new(entity_type.to_string()),
    ];
    if let Some(pid) = &filter.project_id {
        sql.push_str(" AND project_id = ?3");
        params.push(Box::new(pid.clone()));
    }
    sql.push_str(" ORDER BY created_at DESC");
    sql.push_str(" LIMIT ?");
    sql.push_str(&format!("{}", params.len() + 1));
    params.push(Box::new(limit));
    sql.push_str(" OFFSET ?");
    sql.push_str(&format!("{}", params.len() + 1));
    params.push(Box::new(offset));

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt
        .query_map(rusqlite::params_from_iter(refs.into_iter()), row_to_entity)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub fn update(db: &DbPool, id: &str, data: &Value) -> Result<PluginEntity, String> {
    let conn = db.get().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    let data_str = serde_json::to_string(data)
        .map_err(|e| format!("failed to serialize entity data: {}", e))?;
    let affected = conn
        .execute(
            "UPDATE plugin_entities SET data = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![data_str, now, id],
        )
        .map_err(|e| format!("update plugin_entity: {}", e))?;
    if affected == 0 {
        return Err(format!("plugin_entity {:?} not found", id));
    }
    get(db, id)?.ok_or_else(|| format!("plugin_entity {:?} not found after update", id))
}

pub fn delete(db: &DbPool, id: &str) -> Result<(), String> {
    let conn = db.get().map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM plugin_entity_relations WHERE from_id = ?1 OR to_id = ?1",
        rusqlite::params![id],
    )
    .map_err(|e| format!("delete plugin_entity_relations: {}", e))?;
    conn.execute(
        "DELETE FROM plugin_entities WHERE id = ?1",
        rusqlite::params![id],
    )
    .map_err(|e| format!("delete plugin_entity: {}", e))?;
    Ok(())
}

pub fn link(
    db: &DbPool,
    from_kind: &str,
    from_type: &str,
    from_id: &str,
    to_kind: &str,
    to_type: &str,
    to_id: &str,
    relation: &str,
) -> Result<PluginEntityRelation, String> {
    let conn = db.get().map_err(|e| e.to_string())?;
    let id = Ulid::new().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO plugin_entity_relations
            (id, from_kind, from_type, from_id, to_kind, to_type, to_id, relation, created_at, tenant_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, COALESCE((SELECT tenant_id FROM plugin_entities WHERE id = ?4), (SELECT tenant_id FROM plugin_entities WHERE id = ?7), 'local'))
         ON CONFLICT(from_id, to_id, relation) DO NOTHING",
        rusqlite::params![
            id, from_kind, from_type, from_id, to_kind, to_type, to_id, relation, now
        ],
    )
    .map_err(|e| format!("insert plugin_entity_relation: {}", e))?;
    Ok(PluginEntityRelation {
        id,
        from_kind: from_kind.to_string(),
        from_type: from_type.to_string(),
        from_id: from_id.to_string(),
        to_kind: to_kind.to_string(),
        to_type: to_type.to_string(),
        to_id: to_id.to_string(),
        relation: relation.to_string(),
        created_at: now,
    })
}

pub fn unlink(db: &DbPool, from_id: &str, to_id: &str, relation: &str) -> Result<(), String> {
    let conn = db.get().map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM plugin_entity_relations
         WHERE from_id = ?1 AND to_id = ?2 AND relation = ?3",
        rusqlite::params![from_id, to_id, relation],
    )
    .map_err(|e| format!("delete plugin_entity_relation: {}", e))?;
    Ok(())
}

pub fn list_relations(db: &DbPool, entity_id: &str) -> Result<Vec<PluginEntityRelation>, String> {
    let conn = db.get().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, from_kind, from_type, from_id, to_kind, to_type, to_id, relation, created_at
             FROM plugin_entity_relations
             WHERE from_id = ?1 OR to_id = ?1",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![entity_id], |row| {
            Ok(PluginEntityRelation {
                id: row.get(0)?,
                from_kind: row.get(1)?,
                from_type: row.get(2)?,
                from_id: row.get(3)?,
                to_kind: row.get(4)?,
                to_type: row.get(5)?,
                to_id: row.get(6)?,
                relation: row.get(7)?,
                created_at: row.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn row_to_entity(row: &rusqlite::Row<'_>) -> rusqlite::Result<PluginEntity> {
    let data_str: String = row.get(4)?;
    let data = serde_json::from_str::<Value>(&data_str).unwrap_or(Value::Null);
    Ok(PluginEntity {
        id: row.get(0)?,
        plugin_id: row.get(1)?,
        entity_type: row.get(2)?,
        project_id: row.get(3)?,
        data,
        created_by_agent_id: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}
