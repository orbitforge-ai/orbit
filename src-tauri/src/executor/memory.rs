use serde::{Deserialize, Serialize};

/// HTTP client for the memory sidecar service.
///
/// All memory operations go through this client. The backend (self-hosted mem0
/// + FAISS) can be swapped by changing the `base_url` — no other Rust code
/// needs to change.
#[derive(Clone)]
pub struct MemoryClient {
    base_url: String,
    client: reqwest::Client,
}

// ─── Request / Response types ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub text: String,
    pub memory_type: String,
    pub user_id: String,
    pub agent_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub source: String,
    pub score: Option<f64>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct AddMemoryBody {
    text: String,
    memory_type: String,
    user_id: String,
    agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct SearchMemoryBody {
    query: String,
    user_id: String,
    agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    memory_type: Option<String>,
    limit: u32,
}

#[derive(Debug, Serialize)]
struct UpdateMemoryBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct ExtractMemoriesBody {
    conversation_text: String,
    user_id: String,
    agent_id: String,
}

#[derive(Debug, Deserialize)]
struct HealthResponse {
    status: String,
}

// ─── Client implementation ───────────────────────────────────────────────────

impl MemoryClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("failed to build reqwest client"),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/v1/memory{}", self.base_url, path)
    }

    /// Check if the memory service is healthy.
    pub async fn health_check(&self) -> Result<bool, String> {
        let resp = self
            .client
            .get(self.url("/health"))
            .send()
            .await
            .map_err(|e| format!("health check request failed: {}", e))?;

        if !resp.status().is_success() {
            return Ok(false);
        }

        let body: HealthResponse = resp
            .json()
            .await
            .map_err(|e| format!("health check parse failed: {}", e))?;

        Ok(body.status == "ok")
    }

    /// Add a new memory.
    pub async fn add_memory(
        &self,
        text: &str,
        memory_type: &str,
        user_id: &str,
        agent_id: &str,
        metadata: Option<serde_json::Value>,
    ) -> Result<Vec<MemoryEntry>, String> {
        let body = AddMemoryBody {
            text: text.to_string(),
            memory_type: memory_type.to_string(),
            user_id: user_id.to_string(),
            agent_id: agent_id.to_string(),
            metadata,
        };

        let resp = self
            .client
            .post(self.url("/add"))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("add_memory request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("add_memory failed ({}): {}", status, text));
        }

        resp.json()
            .await
            .map_err(|e| format!("add_memory parse failed: {}", e))
    }

    /// Semantic search for memories.
    pub async fn search_memories(
        &self,
        query: &str,
        user_id: &str,
        agent_id: &str,
        memory_type: Option<&str>,
        limit: u32,
    ) -> Result<Vec<MemoryEntry>, String> {
        let body = SearchMemoryBody {
            query: query.to_string(),
            user_id: user_id.to_string(),
            agent_id: agent_id.to_string(),
            memory_type: memory_type.map(|s| s.to_string()),
            limit,
        };

        let resp = self
            .client
            .post(self.url("/search"))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("search_memories request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("search_memories failed ({}): {}", status, text));
        }

        resp.json()
            .await
            .map_err(|e| format!("search_memories parse failed: {}", e))
    }

    /// List memories for a user/agent.
    pub async fn list_memories(
        &self,
        user_id: &str,
        agent_id: &str,
        memory_type: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<MemoryEntry>, String> {
        let mut url = format!(
            "{}?user_id={}&agent_id={}&limit={}&offset={}",
            self.url("/list"),
            urlencoded(user_id),
            urlencoded(agent_id),
            limit,
            offset,
        );
        if let Some(mt) = memory_type {
            url.push_str(&format!("&memory_type={}", urlencoded(mt)));
        }

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("list_memories request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("list_memories failed ({}): {}", status, text));
        }

        resp.json()
            .await
            .map_err(|e| format!("list_memories parse failed: {}", e))
    }

    /// Delete a memory by ID.
    pub async fn delete_memory(&self, memory_id: &str) -> Result<(), String> {
        let resp = self
            .client
            .delete(self.url(&format!("/delete/{}", urlencoded(memory_id))))
            .send()
            .await
            .map_err(|e| format!("delete_memory request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("delete_memory failed ({}): {}", status, text));
        }

        Ok(())
    }

    /// Update a memory's text or metadata.
    pub async fn update_memory(
        &self,
        memory_id: &str,
        text: Option<&str>,
        metadata: Option<serde_json::Value>,
    ) -> Result<MemoryEntry, String> {
        let body = UpdateMemoryBody {
            text: text.map(|s| s.to_string()),
            metadata,
        };

        let resp = self
            .client
            .put(self.url(&format!("/update/{}", urlencoded(memory_id))))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("update_memory request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("update_memory failed ({}): {}", status, text));
        }

        resp.json()
            .await
            .map_err(|e| format!("update_memory parse failed: {}", e))
    }

    /// Auto-extract memories from a conversation.
    pub async fn extract_memories(
        &self,
        conversation_text: &str,
        user_id: &str,
        agent_id: &str,
    ) -> Result<Vec<MemoryEntry>, String> {
        let body = ExtractMemoriesBody {
            conversation_text: conversation_text.to_string(),
            user_id: user_id.to_string(),
            agent_id: agent_id.to_string(),
        };

        let resp = self
            .client
            .post(self.url("/extract"))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("extract_memories request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("extract_memories failed ({}): {}", status, text));
        }

        resp.json()
            .await
            .map_err(|e| format!("extract_memories parse failed: {}", e))
    }
}

/// Minimal percent-encoding for query parameters.
fn urlencoded(s: &str) -> String {
    s.replace('%', "%25")
        .replace(' ', "%20")
        .replace('&', "%26")
        .replace('=', "%3D")
        .replace('+', "%2B")
        .replace('#', "%23")
}
