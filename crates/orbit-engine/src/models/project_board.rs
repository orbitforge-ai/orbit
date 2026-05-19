use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BoardMovementColumnRule {
    pub column_id: String,
    pub purpose: String,
    pub move_when: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BoardMovementTransition {
    pub from_column_id: String,
    pub to_column_id: String,
    pub when: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BoardMovementGuide {
    pub version: u8,
    pub summary: String,
    pub column_rules: Vec<BoardMovementColumnRule>,
    pub transitions: Vec<BoardMovementTransition>,
    pub agent_instructions: String,
}

impl Default for BoardMovementGuide {
    fn default() -> Self {
        Self {
            version: 1,
            summary: String::new(),
            column_rules: Vec::new(),
            transitions: Vec::new(),
            agent_instructions: String::new(),
        }
    }
}

pub fn parse_board_movement_guide(raw: Option<&str>) -> BoardMovementGuide {
    raw.and_then(|value| serde_json::from_str(value).ok())
        .unwrap_or_default()
}

pub fn board_movement_guide_json(guide: &BoardMovementGuide) -> Result<String, String> {
    serde_json::to_string(guide).map_err(|e| e.to_string())
}

pub fn default_board_movement_guide_json() -> String {
    board_movement_guide_json(&BoardMovementGuide::default()).unwrap_or_else(|_| "{}".to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBoard {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub prefix: String,
    pub movement_guide: BoardMovementGuide,
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
    pub movement_guide: Option<BoardMovementGuide>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeleteProjectBoard {
    pub destination_board_id: Option<String>,
    pub force: Option<bool>,
}
