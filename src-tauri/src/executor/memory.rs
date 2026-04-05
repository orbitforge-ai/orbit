use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Constant agent_id sent to mem0 cloud so all agents share a single memory pool.
const GLOBAL_AGENT_ID: &str = "global";

/// HTTP client for the mem0 cloud API.
///
/// All memory operations call https://api.mem0.ai directly. The user_id
/// scoping enforces per-user isolation; the API key is a shared org-level credential
/// embedded at build time.
#[derive(Clone)]
pub struct MemoryClient {
    api_key: String,
    client: reqwest::Client,
}

// ─── Public response type ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEntry {
    pub id: String,
    pub text: String,
    pub memory_type: String,
    pub user_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub source: String,
    pub score: Option<f64>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

fn infer_memory_type(text: &str) -> String {
    let normalized = text.trim().to_lowercase();

    let user_markers = [
        "user's ",
        "the user ",
        "favorite ",
        "prefers ",
        "likes ",
        "dislikes ",
        "allergic ",
        "birthday ",
        "name is ",
        "lives in ",
    ];
    if user_markers.iter().any(|marker| normalized.contains(marker)) {
        return "user".to_string();
    }

    let feedback_markers = [
        "answer ",
        "respond ",
        "response ",
        "tone ",
        "format ",
        "be more ",
        "be less ",
        "avoid ",
        "use bullets",
        "be concise",
        "be brief",
    ];
    if feedback_markers.iter().any(|marker| normalized.contains(marker)) {
        return "feedback".to_string();
    }

    let project_markers = [
        "project ",
        "repo ",
        "repository ",
        "workspace ",
        "branch ",
        "file ",
        "src/",
        "package.json",
        "tauri",
        "api ",
        "database ",
        "schema ",
        "migration ",
        "component ",
        "feature ",
        "bug ",
        "task ",
        "implementation ",
    ];
    if project_markers.iter().any(|marker| normalized.contains(marker)) {
        return "project".to_string();
    }

    "reference".to_string()
}

// ─── Cloud wire types (private) ──────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct CloudAddBody {
    messages: Vec<CloudMessage>,
    user_id: String,
    agent_id: String,
    metadata: serde_json::Value,
    async_mode: bool,
    output_format: &'static str,
}

#[derive(Debug, Serialize)]
struct CloudMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct CloudSearchBody {
    query: String,
    user_id: String,
    agent_id: String,
    top_k: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    filters: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct CloudListBody {
    user_id: String,
    agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    filters: Option<serde_json::Value>,
    page: u32,
    page_size: u32,
}

#[derive(Debug, Serialize)]
struct CloudUpdateBody {
    text: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CloudResponse {
    Wrapped { results: Vec<CloudMemoryItem> },
    Bare(Vec<CloudMemoryItem>),
}

impl CloudResponse {
    fn into_results(self) -> Vec<CloudMemoryItem> {
        match self {
            Self::Wrapped { results } => results,
            Self::Bare(results) => results,
        }
    }
}

#[derive(Debug, Deserialize)]
struct CloudMemoryItem {
    id: String,
    memory: String,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    score: Option<f64>,
    #[serde(default)]
    metadata: serde_json::Value,
    #[serde(default)]
    event: Option<String>,
}

/// The update endpoint returns the item directly, not wrapped in {results:[...]}.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CloudUpdateResult {
    Wrapped {
        results: Vec<CloudMemoryItem>,
    },
    Direct {
        id: String,
        memory: String,
        #[serde(default)]
        metadata: serde_json::Value,
    },
}

// ─── Client implementation ───────────────────────────────────────────────────

impl MemoryClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("failed to build reqwest client"),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("https://api.mem0.ai{}", path)
    }

    fn auth(&self) -> String {
        format!("Token {}", self.api_key)
    }

    /// Add a new memory.
    pub async fn add_memory(
        &self,
        text: &str,
        memory_type: &str,
        user_id: &str,
        extra_metadata: Option<serde_json::Value>,
    ) -> Result<Vec<MemoryEntry>, String> {
        let now = Utc::now().to_rfc3339();
        let mut meta = serde_json::json!({
            "memory_type": memory_type,
            "source": "explicit",
            "created_at": now,
            "updated_at": now,
        });
        if let Some(extra) = extra_metadata {
            if let (Some(m), Some(e)) = (meta.as_object_mut(), extra.as_object()) {
                for (k, v) in e {
                    m.insert(k.clone(), v.clone());
                }
            }
        }

        let body = CloudAddBody {
            messages: vec![CloudMessage {
                role: "user".to_string(),
                content: text.to_string(),
            }],
            user_id: user_id.to_string(),
            agent_id: GLOBAL_AGENT_ID.to_string(),
            metadata: meta,
            async_mode: false,
            output_format: "v1.1",
        };

        let resp = self
            .client
            .post(self.url("/v1/memories/"))
            .header("Authorization", self.auth())
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("add_memory request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("add_memory failed ({}): {}", status, text));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| format!("add_memory body read failed: {}", e))?;
        let cloud = parse_cloud_response(&body, "add_memory")?;

        let entries = cloud
            .into_results()
            .into_iter()
            .filter(|item| {
                // Keep ADD, UPDATE, and items with no event field (some API versions omit it)
                matches!(
                    item.event.as_deref(),
                    Some("ADD") | Some("add") | Some("UPDATE") | Some("update") | None
                ) && !item.id.is_empty()
            })
            .map(|item| cloud_item_to_entry(item, user_id))
            .collect();

        Ok(entries)
    }

    /// Semantic search for memories.
    pub async fn search_memories(
        &self,
        query: &str,
        user_id: &str,
        memory_type: Option<&str>,
        limit: u32,
    ) -> Result<Vec<MemoryEntry>, String> {
        let body = CloudSearchBody {
            query: query.to_string(),
            user_id: user_id.to_string(),
            agent_id: GLOBAL_AGENT_ID.to_string(),
            top_k: limit,
            filters: None,
        };

        let resp = self
            .client
            .post(self.url("/v2/memories/search/"))
            .header("Authorization", self.auth())
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("search_memories request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("search_memories failed ({}): {}", status, text));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| format!("search_memories body read failed: {}", e))?;
        let cloud = parse_cloud_response(&body, "search_memories")?;

        let mut entries: Vec<MemoryEntry> = cloud
            .into_results()
            .into_iter()
            .map(|item| cloud_item_to_entry(item, user_id))
            .collect();

        // Post-filter by memory_type if requested (cloud may not filter metadata natively)
        if let Some(mt) = memory_type {
            entries.retain(|e| e.memory_type == mt);
        }

        Ok(entries)
    }

    /// List memories for a user.
    pub async fn list_memories(
        &self,
        user_id: &str,
        memory_type: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<MemoryEntry>, String> {
        // Map offset+limit → 1-indexed page (works exactly when offset is a multiple of limit)
        let page = (offset / limit.max(1)) + 1;

        let body = CloudListBody {
            user_id: user_id.to_string(),
            agent_id: GLOBAL_AGENT_ID.to_string(),
            filters: None,
            page,
            page_size: limit,
        };

        let resp = self
            .client
            .post(self.url("/v2/memories/"))
            .header("Authorization", self.auth())
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("list_memories request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("list_memories failed ({}): {}", status, text));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| format!("list_memories body read failed: {}", e))?;
        let cloud = parse_cloud_response(&body, "list_memories")?;

        let mut entries: Vec<MemoryEntry> = cloud
            .into_results()
            .into_iter()
            .map(|item| cloud_item_to_entry(item, user_id))
            .collect();

        if let Some(mt) = memory_type {
            entries.retain(|e| e.memory_type == mt);
        }

        Ok(entries)
    }

    /// Delete a memory by ID.
    pub async fn delete_memory(&self, memory_id: &str) -> Result<(), String> {
        let resp = self
            .client
            .delete(self.url(&format!("/v1/memories/{}/", memory_id)))
            .header("Authorization", self.auth())
            .send()
            .await
            .map_err(|e| format!("delete_memory request failed: {}", e))?;

        // Accept 200 or 204
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("delete_memory failed ({}): {}", status, text));
        }

        Ok(())
    }

    /// Update a memory's text.
    pub async fn update_memory(
        &self,
        memory_id: &str,
        text: Option<&str>,
        _metadata: Option<serde_json::Value>,
    ) -> Result<MemoryEntry, String> {
        let text_val = text.unwrap_or("");
        let body = CloudUpdateBody {
            text: text_val.to_string(),
        };

        let resp = self
            .client
            .put(self.url(&format!("/v1/memories/{}/", memory_id)))
            .header("Authorization", self.auth())
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("update_memory request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("update_memory failed ({}): {}", status, text));
        }

        let result: CloudUpdateResult = resp
            .json()
            .await
            .map_err(|e| format!("update_memory parse failed: {}", e))?;

        let item = match result {
            CloudUpdateResult::Wrapped { results } => results
                .into_iter()
                .next()
                .ok_or_else(|| "update_memory: empty results".to_string())?,
            CloudUpdateResult::Direct {
                id,
                memory,
                metadata,
            } => CloudMemoryItem {
                id,
                memory,
                created_at: None,
                updated_at: None,
                score: None,
                metadata,
                event: None,
            },
        };

        Ok(cloud_item_to_entry(item, ""))
    }

    /// Auto-extract memories from a conversation.
    /// Delegates to the same add endpoint — mem0 cloud runs its own extraction pipeline.
    pub async fn extract_memories(
        &self,
        conversation_text: &str,
        user_id: &str,
    ) -> Result<Vec<MemoryEntry>, String> {
        let now = Utc::now().to_rfc3339();
        let meta = serde_json::json!({
            "source": "auto_extracted",
            "created_at": now,
            "updated_at": now,
        });

        let body = CloudAddBody {
            messages: vec![CloudMessage {
                role: "user".to_string(),
                content: conversation_text.to_string(),
            }],
            user_id: user_id.to_string(),
            agent_id: GLOBAL_AGENT_ID.to_string(),
            metadata: meta,
            async_mode: false,
            output_format: "v1.1",
        };

        let resp = self
            .client
            .post(self.url("/v1/memories/"))
            .header("Authorization", self.auth())
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("extract_memories request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("extract_memories failed ({}): {}", status, text));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| format!("extract_memories body read failed: {}", e))?;
        let cloud = parse_cloud_response(&body, "extract_memories")?;

        let entries = cloud
            .into_results()
            .into_iter()
            .filter(|item| {
                matches!(
                    item.event.as_deref(),
                    Some("ADD") | Some("add") | Some("UPDATE") | Some("update") | None
                ) && !item.id.is_empty()
            })
            .map(|item| cloud_item_to_entry(item, user_id))
            .collect();

        Ok(entries)
    }
}

// ─── Mapping helper ──────────────────────────────────────────────────────────

fn parse_cloud_response(body: &str, operation: &str) -> Result<CloudResponse, String> {
    serde_json::from_str::<CloudResponse>(body).map_err(|e| {
        let snippet: String = body.chars().take(240).collect();
        format!("{operation} parse failed: {e}. response body starts with: {snippet}")
    })
}

fn cloud_item_to_entry(item: CloudMemoryItem, user_id: &str) -> MemoryEntry {
    let meta = &item.metadata;

    let source = meta
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("explicit")
        .to_string();
    let raw_memory_type = meta
        .get("memory_type")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let created_at = meta
        .get("created_at")
        .and_then(|v| v.as_str())
        .or(item.created_at.as_deref())
        .unwrap_or("")
        .to_string();
    let updated_at = meta
        .get("updated_at")
        .and_then(|v| v.as_str())
        .or(item.updated_at.as_deref())
        .unwrap_or("")
        .to_string();
    let memory_type = match (source.as_str(), raw_memory_type.as_deref()) {
        ("auto_extracted", _) => infer_memory_type(&item.memory),
        (_, Some(mt)) => mt.to_string(),
        _ => infer_memory_type(&item.memory),
    };

    // Strip known fields from residual metadata
    let residual_metadata = match &item.metadata {
        serde_json::Value::Object(map) => {
            let filtered: serde_json::Map<_, _> = map
                .iter()
                .filter(|(k, _)| {
                    !matches!(
                        k.as_str(),
                        "memory_type" | "source" | "created_at" | "updated_at"
                    )
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            serde_json::Value::Object(filtered)
        }
        other => other.clone(),
    };

    MemoryEntry {
        id: item.id,
        text: item.memory,
        memory_type,
        user_id: user_id.to_string(),
        created_at,
        updated_at,
        source,
        score: item.score,
        metadata: residual_metadata,
    }
}

#[cfg(test)]
mod tests {
    use super::{cloud_item_to_entry, infer_memory_type, parse_cloud_response, MemoryEntry};
    use serde_json::json;

    #[test]
    fn parses_wrapped_cloud_response() {
        let body = r#"{
            "results": [
                {
                    "id": "mem_1",
                    "memory": "Wrapped response",
                    "metadata": {
                        "memory_type": "project",
                        "created_at": "2026-04-03T12:00:00Z",
                        "updated_at": "2026-04-03T12:00:00Z"
                    }
                }
            ]
        }"#;

        let parsed =
            parse_cloud_response(body, "search_memories").expect("wrapped response should parse");
        let results = parsed.into_results();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "mem_1");
    }

    #[test]
    fn parses_bare_array_response_and_uses_top_level_timestamps() {
        let body = r#"[
            {
                "id": "mem_2",
                "memory": "Bare response",
                "created_at": "2026-04-03T12:00:00Z",
                "updated_at": "2026-04-03T13:00:00Z",
                "metadata": {
                    "memory_type": "reference"
                }
            }
        ]"#;

        let parsed =
            parse_cloud_response(body, "search_memories").expect("bare response should parse");
        let results = parsed.into_results();
        let entry = cloud_item_to_entry(results.into_iter().next().expect("result"), "user_1");

        assert_eq!(entry.text, "Bare response");
        assert_eq!(entry.created_at, "2026-04-03T12:00:00Z");
        assert_eq!(entry.updated_at, "2026-04-03T13:00:00Z");
        assert_eq!(entry.memory_type, "reference");
    }

    #[test]
    fn memory_entry_serializes_with_camel_case_keys() {
        let entry = MemoryEntry {
            id: "mem_4".to_string(),
            text: "Camel case".to_string(),
            memory_type: "user".to_string(),
            user_id: "user_1".to_string(),
            created_at: "2026-04-04T12:00:00Z".to_string(),
            updated_at: "2026-04-04T12:00:00Z".to_string(),
            source: "explicit".to_string(),
            score: Some(0.98),
            metadata: json!({}),
        };

        let value = serde_json::to_value(&entry).expect("memory entry should serialize");

        assert_eq!(value["memoryType"], "user");
        assert_eq!(value["createdAt"], "2026-04-04T12:00:00Z");
        assert_eq!(value["updatedAt"], "2026-04-04T12:00:00Z");
        assert_eq!(value["userId"], "user_1");
        assert!(value.get("memory_type").is_none());
        assert!(value.get("created_at").is_none());
        assert!(value.get("agentId").is_none());
    }

    #[test]
    fn infer_memory_type_detects_user_facts() {
        assert_eq!(
            infer_memory_type("User's favorite color is red."),
            "user"
        );
    }

    #[test]
    fn auto_extracted_entries_are_reclassified_from_text() {
        let item = super::CloudMemoryItem {
            id: "mem_5".to_string(),
            memory: "User's favorite color is red.".to_string(),
            created_at: Some("2026-04-04T12:00:00Z".to_string()),
            updated_at: Some("2026-04-04T12:00:00Z".to_string()),
            score: None,
            metadata: json!({
                "memory_type": "project",
                "source": "auto_extracted"
            }),
            event: None,
        };

        let entry = cloud_item_to_entry(item, "user_1");

        assert_eq!(entry.memory_type, "user");
    }
}
