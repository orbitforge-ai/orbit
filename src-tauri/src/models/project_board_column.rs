use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBoardColumn {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub status: String,
    pub position: f64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectBoardColumn {
    pub project_id: String,
    pub name: String,
    pub status: String,
    pub position: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProjectBoardColumn {
    pub name: Option<String>,
    pub status: Option<String>,
    pub position: Option<f64>,
}
