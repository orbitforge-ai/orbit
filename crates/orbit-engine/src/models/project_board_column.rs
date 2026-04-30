use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBoardColumn {
    pub id: String,
    pub project_id: String,
    pub board_id: String,
    pub name: String,
    pub role: Option<String>,
    pub is_default: bool,
    pub position: f64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectBoardColumn {
    pub project_id: String,
    pub board_id: Option<String>,
    pub name: String,
    pub role: Option<String>,
    pub is_default: Option<bool>,
    pub position: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProjectBoardColumn {
    pub name: Option<String>,
    #[serde(default)]
    pub role: Option<Option<String>>,
    pub is_default: Option<bool>,
    pub position: Option<f64>,
    pub expected_revision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteProjectBoardColumn {
    pub destination_column_id: Option<String>,
    pub force: Option<bool>,
    pub expected_revision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReorderProjectBoardColumns {
    pub board_id: Option<String>,
    pub ordered_ids: Vec<String>,
    pub expected_revision: Option<String>,
}
