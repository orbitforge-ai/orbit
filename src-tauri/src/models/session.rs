use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// Shared environment variables injected into all tasks in this session
    pub environment: std::collections::HashMap<String, String>,
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSession {
    pub name: String,
    pub description: Option<String>,
    pub environment: Option<std::collections::HashMap<String, String>>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSession {
    pub name: Option<String>,
    pub description: Option<String>,
    pub environment: Option<std::collections::HashMap<String, String>>,
    pub tags: Option<Vec<String>>,
}
