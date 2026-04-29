use chrono::Utc;
use rusqlite::OptionalExtension;
use serde_json::Value;
use sha2::{Digest, Sha256};
use ulid::Ulid;

use crate::db::DbPool;

pub(crate) fn hash_text(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub(crate) fn fingerprint_listing(item: &Value, source_key: &str) -> String {
    if let Some(url) = item
        .get("url")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        return hash_text(url);
    }

    let title = item.get("title").and_then(Value::as_str).unwrap_or("");
    let published = item
        .get("publishedAt")
        .and_then(Value::as_str)
        .unwrap_or("");
    let body_hash = item.get("bodyHash").and_then(Value::as_str).unwrap_or("");
    hash_text(&format!(
        "{}|{}|{}|{}",
        source_key, title, published, body_hash
    ))
}

pub(crate) async fn filter_unseen_items(
    db: &DbPool,
    workflow_id: &str,
    node_id: &str,
    source_key: &str,
    items: Vec<Value>,
) -> Result<Vec<Value>, String> {
    let pool = db.0.clone();
    let workflow_id = workflow_id.to_string();
    let node_id = node_id.to_string();
    let source_key = source_key.to_string();
    tokio::task::spawn_blocking(move || -> Result<Vec<Value>, String> {
        let mut conn = pool.get().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let now = Utc::now().to_rfc3339();
        let mut unseen = Vec::new();

        for item in items {
            let fingerprint = fingerprint_listing(&item, &source_key);
            let exists: Option<String> = tx
                .query_row(
                    "SELECT id FROM workflow_seen_items
                     WHERE workflow_id = ?1
                       AND node_id = ?2
                       AND source_key = ?3
                       AND fingerprint = ?4
                       AND tenant_id = COALESCE((SELECT tenant_id FROM project_workflows WHERE id = ?1), 'local')",
                    rusqlite::params![workflow_id, node_id, source_key, fingerprint],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())?;
            if exists.is_some() {
                continue;
            }

            tx.execute(
                "INSERT INTO workflow_seen_items (
                    id, workflow_id, node_id, source_key, fingerprint, created_at, tenant_id
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, COALESCE((SELECT tenant_id FROM project_workflows WHERE id = ?2), 'local'))",
                rusqlite::params![
                    Ulid::new().to_string(),
                    workflow_id,
                    node_id,
                    source_key,
                    fingerprint,
                    now,
                ],
            )
            .map_err(|e| e.to_string())?;
            unseen.push(item);
        }

        tx.commit().map_err(|e| e.to_string())?;
        Ok(unseen)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::{filter_unseen_items, fingerprint_listing, hash_text};
    use crate::db::connection::init as init_db;
    use serde_json::json;
    use std::path::PathBuf;

    fn temp_db_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("orbit-workflows-{}-{}", name, ulid::Ulid::new()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn seed_workflow_fixture(db: &crate::db::DbPool, workflow_id: &str) {
        let conn = db.get().unwrap();
        let now = "2024-01-01T00:00:00Z";
        conn.execute(
            "INSERT INTO projects (id, name, description, created_at, updated_at, tenant_id)
             VALUES (?1, ?2, NULL, ?3, ?3, 'local')",
            rusqlite::params!["project-1", "Test Project", now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO project_workflows
                (id, project_id, name, description, enabled, graph, trigger_kind, trigger_config, version, created_at, updated_at, tenant_id)
             VALUES
                (?1, ?2, ?3, NULL, 1, ?4, 'manual', '{}', 1, ?5, ?5, COALESCE((SELECT tenant_id FROM projects WHERE id = ?2), 'local'))",
            rusqlite::params![
                workflow_id,
                "project-1",
                "Workflow",
                r#"{"nodes":[],"edges":[],"schemaVersion":1}"#,
                now
            ],
        )
        .unwrap();
    }

    #[tokio::test]
    async fn filter_unseen_items_only_returns_new_rows() {
        let dir = temp_db_dir("seen-items");
        let db = init_db(dir.clone()).unwrap();
        seed_workflow_fixture(&db, "wf1");

        let items = vec![
            json!({"url": "https://example.com/a", "title": "A"}),
            json!({"url": "https://example.com/b", "title": "B"}),
        ];

        let first = filter_unseen_items(&db, "wf1", "node1", "feed", items.clone())
            .await
            .unwrap();
        let second = filter_unseen_items(&db, "wf1", "node1", "feed", items)
            .await
            .unwrap();

        assert_eq!(first.len(), 2);
        assert!(second.is_empty());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn fingerprint_prefers_url_then_content_fallback() {
        let with_url = json!({"url": "https://example.com/post"});
        assert_eq!(
            fingerprint_listing(&with_url, "feed"),
            hash_text("https://example.com/post")
        );

        let fallback = json!({
            "title": "Hello",
            "publishedAt": "2024-01-01T00:00:00Z",
            "bodyHash": "body"
        });
        assert_eq!(
            fingerprint_listing(&fallback, "feed"),
            hash_text("feed|Hello|2024-01-01T00:00:00Z|body")
        );
    }
}
