use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBoard {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub prefix: String,
    pub position: f64,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectBoard {
    pub project_id: String,
    pub name: String,
    pub prefix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProjectBoard {
    pub name: Option<String>,
    pub prefix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeleteProjectBoard {
    pub destination_board_id: Option<String>,
    pub force: Option<bool>,
}
