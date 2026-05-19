//! `workspace_board` agent tool — lets agents manage project board structure.
//! Work item lifecycle remains owned by `work_item`; this tool only changes
//! boards, columns, defaults, ordering, and the movement guide agents consult
//! before moving cards.

use serde_json::{json, Map, Value};
use tauri::Manager;

use crate::app_context::AppContext;
use crate::commands::project_board_columns::{
    create_project_board_column_inner, delete_project_board_column_inner,
    reorder_project_board_columns_inner, update_project_board_column_inner,
};
use crate::executor::llm_provider::ToolDefinition;
use crate::models::project_board::{
    BoardMovementGuide, CreateProjectBoard, DeleteProjectBoard, ProjectBoard, UpdateProjectBoard,
};
use crate::models::project_board_column::{
    CreateProjectBoardColumn, DeleteProjectBoardColumn, ProjectBoardColumn,
    ReorderProjectBoardColumns, UpdateProjectBoardColumn,
};

use super::{context::ToolExecutionContext, ToolHandler};

pub struct WorkspaceBoardTool;

#[async_trait::async_trait]
impl ToolHandler for WorkspaceBoardTool {
    fn name(&self) -> &'static str {
        "workspace_board"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Manage project workspace boards, role-free columns, defaults, ordering, and board movement guides. Use `work_item` for cards and lifecycle status changes. Actions: list, get, create_board, update_board, set_default_board, delete_board, create_column, update_column, set_default_column, delete_column, reorder_columns, get_movement_guide, update_movement_guide. `project_id` is inferred from the current session when omitted. Columns do not have roles; encode movement semantics in the board movement guide.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": [
                            "list", "get",
                            "create_board", "update_board", "set_default_board", "delete_board",
                            "create_column", "update_column", "set_default_column", "delete_column", "reorder_columns",
                            "get_movement_guide", "update_movement_guide"
                        ],
                        "description": "Action to perform"
                    },
                    "project_id": { "type": "string", "description": "Target project. Defaults to the current session's project when omitted." },
                    "board_id": { "type": "string", "description": "Board id. Defaults to the project's default board for get and movement-guide actions." },
                    "column_id": { "type": "string", "description": "Column id for column actions." },
                    "name": { "type": "string", "description": "Board or column name." },
                    "prefix": { "type": "string", "description": "Board prefix, 2-8 uppercase ASCII letters." },
                    "is_default": { "type": "boolean", "description": "Set true when creating/updating a column to make it the default column." },
                    "position": { "type": "number", "description": "Explicit board-column position." },
                    "ordered_column_ids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Complete ordered list of column ids for reorder_columns."
                    },
                    "movement_guide": {
                        "type": "object",
                        "description": "BoardMovementGuide object. Use camelCase fields: version, summary, columnRules, transitions, agentInstructions.",
                        "properties": {
                            "version": { "type": "integer", "enum": [1] },
                            "summary": { "type": "string" },
                            "columnRules": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "columnId": { "type": "string" },
                                        "purpose": { "type": "string" },
                                        "moveWhen": { "type": "string" }
                                    },
                                    "required": ["columnId", "purpose", "moveWhen"]
                                }
                            },
                            "transitions": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "fromColumnId": { "type": "string" },
                                        "toColumnId": { "type": "string" },
                                        "when": { "type": "string" }
                                    },
                                    "required": ["fromColumnId", "toColumnId", "when"]
                                }
                            },
                            "agentInstructions": { "type": "string" }
                        },
                        "required": ["version", "summary", "columnRules", "transitions", "agentInstructions"]
                    },
                    "destination_board_id": { "type": "string", "description": "Destination board for delete_board when re-parenting contents." },
                    "destination_column_id": { "type": "string", "description": "Destination column for delete_column when reassigning cards." },
                    "force": { "type": "boolean", "description": "Allow guarded deletion when existing references/items require force." },
                    "expected_revision": { "type": "string", "description": "Optimistic board-column revision from the latest column list." }
                },
                "required": ["action"]
            }),
        }
    }

    async fn execute(
        &self,
        ctx: &ToolExecutionContext,
        input: &Value,
        app: &tauri::AppHandle,
        _run_id: &str,
    ) -> Result<(String, bool), String> {
        let repos = ctx
            .repos
            .as_ref()
            .ok_or("workspace_board: no repositories available")?;
        let action = input["action"]
            .as_str()
            .ok_or("workspace_board: missing 'action' field")?;

        match action {
            "list" => {
                let project_id = resolve_project_id(ctx, input, None).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let boards = repos.project_boards().list(&project_id).await?;
                json_result(json!({
                    "status": "ok",
                    "project_id": project_id,
                    "boards": boards,
                }))
            }
            "get" => {
                let project_id = resolve_project_id(ctx, input, None).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let board = resolve_board(ctx, &project_id, input["board_id"].as_str()).await?;
                let columns = repos
                    .project_board_columns()
                    .list(&project_id, Some(board.id.clone()))
                    .await?;
                json_result(board_result("ok", board, Some(columns)))
            }
            "create_board" => {
                let project_id = resolve_project_id(ctx, input, None).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let name = required_str(input, "name", action)?.to_string();
                let prefix = required_str(input, "prefix", action)?.to_string();
                let board = repos
                    .project_boards()
                    .create(CreateProjectBoard {
                        project_id,
                        name,
                        prefix,
                    })
                    .await?;
                spawn_cloud_upsert_board(ctx, &board);
                json_result(board_result("created", board, None))
            }
            "update_board" => {
                let board_id = required_str(input, "board_id", action)?;
                let project_id = resolve_project_for_board(ctx, board_id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let movement_guide = optional_movement_guide(input)?;
                let board = repos
                    .project_boards()
                    .update(
                        board_id,
                        UpdateProjectBoard {
                            name: optional_trimmed(input.get("name")),
                            prefix: optional_trimmed(input.get("prefix")),
                            movement_guide,
                        },
                    )
                    .await?;
                spawn_cloud_upsert_board(ctx, &board);
                json_result(board_result("updated", board, None))
            }
            "set_default_board" => {
                let board_id = required_str(input, "board_id", action)?;
                let project_id = resolve_project_for_board(ctx, board_id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let boards = repos.project_boards().set_default(board_id).await?;
                for board in &boards {
                    spawn_cloud_upsert_board(ctx, board);
                }
                let default_board = boards.iter().find(|board| board.is_default).cloned();
                json_result(json!({
                    "status": "default_board_set",
                    "project_id": project_id,
                    "default_board": default_board,
                    "boards": boards,
                }))
            }
            "delete_board" => {
                let board_id = required_str(input, "board_id", action)?;
                let project_id = resolve_project_for_board(ctx, board_id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                repos
                    .project_boards()
                    .delete(
                        board_id,
                        DeleteProjectBoard {
                            destination_board_id: optional_trimmed(
                                input.get("destination_board_id"),
                            ),
                            force: input["force"].as_bool(),
                        },
                    )
                    .await?;
                spawn_cloud_delete(ctx, "project_boards", board_id);
                let boards = repos.project_boards().list(&project_id).await?;
                for board in &boards {
                    spawn_cloud_upsert_board(ctx, board);
                }
                json_result(json!({
                    "status": "deleted",
                    "id": board_id,
                    "project_id": project_id,
                    "boards": boards,
                }))
            }
            "create_column" => {
                reject_role_input(input, action)?;
                let project_id = resolve_project_id(ctx, input, None).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let app_ctx = app.state::<AppContext>();
                let column = create_project_board_column_inner(
                    CreateProjectBoardColumn {
                        project_id: project_id.clone(),
                        board_id: optional_trimmed(input.get("board_id")),
                        name: required_str(input, "name", action)?.to_string(),
                        is_default: input["is_default"].as_bool(),
                        position: input["position"].as_f64(),
                    },
                    &app_ctx,
                )
                .await?;
                if column.is_default {
                    sync_columns_for_board(ctx, &project_id, &column.board_id).await?;
                }
                json_result(column_result("created", column))
            }
            "update_column" => {
                reject_role_input(input, action)?;
                let column_id = required_str(input, "column_id", action)?;
                let project_id = resolve_project_for_column(ctx, column_id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let app_ctx = app.state::<AppContext>();
                let column = update_project_board_column_inner(
                    column_id.to_string(),
                    UpdateProjectBoardColumn {
                        name: optional_trimmed(input.get("name")),
                        is_default: input["is_default"].as_bool(),
                        position: input["position"].as_f64(),
                        expected_revision: optional_trimmed(input.get("expected_revision")),
                    },
                    &app_ctx,
                )
                .await?;
                if column.is_default {
                    sync_columns_for_board(ctx, &project_id, &column.board_id).await?;
                }
                json_result(column_result("updated", column))
            }
            "set_default_column" => {
                let column_id = required_str(input, "column_id", action)?;
                let project_id = resolve_project_for_column(ctx, column_id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let app_ctx = app.state::<AppContext>();
                let column = update_project_board_column_inner(
                    column_id.to_string(),
                    UpdateProjectBoardColumn {
                        is_default: Some(true),
                        expected_revision: optional_trimmed(input.get("expected_revision")),
                        ..Default::default()
                    },
                    &app_ctx,
                )
                .await?;
                sync_columns_for_board(ctx, &project_id, &column.board_id).await?;
                json_result(column_result("default_column_set", column))
            }
            "delete_column" => {
                let column_id = required_str(input, "column_id", action)?;
                let project_id = resolve_project_for_column(ctx, column_id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let existing = repos
                    .project_board_columns()
                    .get(column_id)
                    .await?
                    .ok_or_else(|| format!("workspace_board: column '{}' not found", column_id))?;
                let app_ctx = app.state::<AppContext>();
                delete_project_board_column_inner(
                    column_id.to_string(),
                    DeleteProjectBoardColumn {
                        destination_column_id: optional_trimmed(input.get("destination_column_id")),
                        force: input["force"].as_bool(),
                        expected_revision: optional_trimmed(input.get("expected_revision")),
                    },
                    &app_ctx,
                )
                .await?;
                let columns = repos
                    .project_board_columns()
                    .list(&project_id, Some(existing.board_id.clone()))
                    .await?;
                for column in &columns {
                    spawn_cloud_upsert_column(ctx, column);
                }
                json_result(json!({
                    "status": "deleted",
                    "id": column_id,
                    "project_id": project_id,
                    "board_id": existing.board_id,
                    "columns": columns,
                }))
            }
            "reorder_columns" => {
                let project_id = resolve_project_id_from_board_or_session(ctx, input).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let ordered_ids = required_string_array(input, "ordered_column_ids", action)?;
                let app_ctx = app.state::<AppContext>();
                let columns = reorder_project_board_columns_inner(
                    project_id.clone(),
                    ReorderProjectBoardColumns {
                        board_id: optional_trimmed(input.get("board_id")),
                        ordered_ids,
                        expected_revision: optional_trimmed(input.get("expected_revision")),
                    },
                    &app_ctx,
                )
                .await?;
                json_result(json!({
                    "status": "reordered",
                    "project_id": project_id,
                    "columns": columns,
                }))
            }
            "get_movement_guide" => {
                let project_id = resolve_project_id(ctx, input, None).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let board = resolve_board(ctx, &project_id, input["board_id"].as_str()).await?;
                json_result(json!({
                    "status": "ok",
                    "project_id": project_id,
                    "board_id": board.id,
                    "movementGuide": board.movement_guide,
                }))
            }
            "update_movement_guide" => {
                let board_id = required_str(input, "board_id", action)?;
                let project_id = resolve_project_for_board(ctx, board_id).await?;
                enforce_project_scope(ctx, &project_id).await?;
                let movement_guide = required_movement_guide(input)?;
                let board = repos
                    .project_boards()
                    .update(
                        board_id,
                        UpdateProjectBoard {
                            movement_guide: Some(movement_guide),
                            ..Default::default()
                        },
                    )
                    .await?;
                spawn_cloud_upsert_board(ctx, &board);
                json_result(json!({
                    "status": "movement_guide_updated",
                    "project_id": project_id,
                    "board": board,
                }))
            }
            other => Err(format!("workspace_board: unknown action '{}'", other)),
        }
    }
}

fn required_str<'a>(input: &'a Value, field: &str, action: &str) -> Result<&'a str, String> {
    input[field]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("workspace_board: {} requires '{}'", action, field))
}

fn optional_trimmed(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

async fn resolve_project_id(
    ctx: &ToolExecutionContext,
    input: &Value,
    explicit: Option<&str>,
) -> Result<String, String> {
    if let Some(project_id) = explicit {
        return Ok(project_id.to_string());
    }
    if let Some(project_id) = input["project_id"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        return Ok(project_id.to_string());
    }
    let Some(session_id) = ctx.current_session_id.as_deref() else {
        return Err(
            "workspace_board: no project_id provided and no current session to infer from".into(),
        );
    };
    let repos = ctx
        .repos
        .as_ref()
        .ok_or("workspace_board: no repositories available")?;
    repos
        .chat()
        .session_meta(session_id)
        .await?
        .project_id
        .ok_or_else(|| {
            "workspace_board: no project_id provided and current session is not scoped to a project"
                .to_string()
        })
}

async fn resolve_project_for_board(
    ctx: &ToolExecutionContext,
    board_id: &str,
) -> Result<String, String> {
    let repos = ctx
        .repos
        .as_ref()
        .ok_or("workspace_board: no repositories available")?;
    repos
        .project_boards()
        .get(board_id)
        .await?
        .map(|board| board.project_id)
        .ok_or_else(|| format!("workspace_board: board '{}' not found", board_id))
}

async fn resolve_project_for_column(
    ctx: &ToolExecutionContext,
    column_id: &str,
) -> Result<String, String> {
    let repos = ctx
        .repos
        .as_ref()
        .ok_or("workspace_board: no repositories available")?;
    repos
        .project_board_columns()
        .get(column_id)
        .await?
        .map(|column| column.project_id)
        .ok_or_else(|| format!("workspace_board: column '{}' not found", column_id))
}

async fn resolve_project_id_from_board_or_session(
    ctx: &ToolExecutionContext,
    input: &Value,
) -> Result<String, String> {
    if let Some(project_id) = input["project_id"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        return Ok(project_id.to_string());
    }
    if let Some(board_id) = input["board_id"].as_str().filter(|value| !value.is_empty()) {
        return resolve_project_for_board(ctx, board_id).await;
    }
    resolve_project_id(ctx, input, None).await
}

async fn enforce_project_scope(ctx: &ToolExecutionContext, project_id: &str) -> Result<(), String> {
    let repos = ctx
        .repos
        .as_ref()
        .ok_or("workspace_board: no repositories available")?;
    let in_project = repos
        .projects()
        .agent_in_project(project_id, &ctx.agent_id)
        .await?;
    if !in_project {
        return Err(serde_json::json!({
            "code": "agent_not_in_project",
            "project_id": project_id,
            "message": format!("agent is not a member of project '{}'", project_id),
        })
        .to_string());
    }
    Ok(())
}

async fn resolve_board(
    ctx: &ToolExecutionContext,
    project_id: &str,
    board_id: Option<&str>,
) -> Result<ProjectBoard, String> {
    let repos = ctx
        .repos
        .as_ref()
        .ok_or("workspace_board: no repositories available")?;
    let boards = repos.project_boards().list(project_id).await?;
    match board_id {
        Some(id) => boards
            .into_iter()
            .find(|board| board.id == id)
            .ok_or_else(|| format!("workspace_board: board '{}' not found", id)),
        None => boards
            .into_iter()
            .find(|board| board.is_default)
            .ok_or_else(|| {
                format!(
                    "workspace_board: project '{}' has no default board",
                    project_id
                )
            }),
    }
}

fn required_string_array(input: &Value, field: &str, action: &str) -> Result<Vec<String>, String> {
    let values = input[field].as_array().ok_or_else(|| {
        format!(
            "workspace_board: {} requires '{}' as an array",
            action, field
        )
    })?;
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .ok_or_else(|| format!("workspace_board: '{}' entries must be strings", field))
        })
        .collect()
}

fn reject_role_input(input: &Value, action: &str) -> Result<(), String> {
    if input.get("role").is_some() {
        return Err(format!(
            "workspace_board: {} does not accept 'role'; columns are role-free, use movement_guide for semantics",
            action
        ));
    }
    Ok(())
}

fn required_movement_guide(input: &Value) -> Result<BoardMovementGuide, String> {
    let value = input
        .get("movement_guide")
        .or_else(|| input.get("movementGuide"))
        .ok_or("workspace_board: update_movement_guide requires 'movement_guide'")?;
    parse_movement_guide(value)
}

fn optional_movement_guide(input: &Value) -> Result<Option<BoardMovementGuide>, String> {
    input
        .get("movement_guide")
        .or_else(|| input.get("movementGuide"))
        .map(parse_movement_guide)
        .transpose()
}

fn parse_movement_guide(value: &Value) -> Result<BoardMovementGuide, String> {
    serde_json::from_value(normalize_movement_guide_value(value.clone()))
        .map_err(|e| format!("workspace_board: invalid movement_guide: {}", e))
}

fn normalize_movement_guide_value(value: Value) -> Value {
    let Value::Object(mut object) = value else {
        return value;
    };
    alias_field(&mut object, "column_rules", "columnRules");
    alias_field(&mut object, "agent_instructions", "agentInstructions");
    if let Some(Value::Array(rules)) = object.get_mut("columnRules") {
        for rule in rules {
            if let Value::Object(rule_object) = rule {
                alias_field(rule_object, "column_id", "columnId");
                alias_field(rule_object, "move_when", "moveWhen");
            }
        }
    }
    if let Some(Value::Array(transitions)) = object.get_mut("transitions") {
        for transition in transitions {
            if let Value::Object(transition_object) = transition {
                alias_field(transition_object, "from_column_id", "fromColumnId");
                alias_field(transition_object, "to_column_id", "toColumnId");
            }
        }
    }
    Value::Object(object)
}

fn alias_field(object: &mut Map<String, Value>, snake: &str, camel: &str) {
    if !object.contains_key(camel) {
        if let Some(value) = object.remove(snake) {
            object.insert(camel.to_string(), value);
        }
    }
}

fn board_result(
    status: &str,
    board: ProjectBoard,
    columns: Option<Vec<ProjectBoardColumn>>,
) -> Value {
    let mut result = json!({
        "status": status,
        "project_id": board.project_id,
        "board": board,
    });
    if let Some(columns) = columns {
        result["columns"] = json!(columns);
    }
    result
}

fn column_result(status: &str, column: ProjectBoardColumn) -> Value {
    json!({
        "status": status,
        "project_id": column.project_id,
        "board_id": column.board_id,
        "column": column,
    })
}

fn json_result(value: Value) -> Result<(String, bool), String> {
    serde_json::to_string_pretty(&value)
        .map(|text| (text, false))
        .map_err(|e| format!("workspace_board: serialize: {}", e))
}

fn spawn_cloud_upsert_board(ctx: &ToolExecutionContext, board: &ProjectBoard) {
    if let Some(client) = ctx.cloud_client.clone() {
        let board = board.clone();
        tokio::spawn(async move {
            if let Err(e) = client.upsert_project_board(&board).await {
                tracing::warn!("cloud upsert project_board (tool): {}", e);
            }
        });
    }
}

fn spawn_cloud_upsert_column(ctx: &ToolExecutionContext, column: &ProjectBoardColumn) {
    if let Some(client) = ctx.cloud_client.clone() {
        let column = column.clone();
        tokio::spawn(async move {
            if let Err(e) = client.upsert_project_board_column(&column).await {
                tracing::warn!("cloud upsert project_board_column (tool): {}", e);
            }
        });
    }
}

async fn sync_columns_for_board(
    ctx: &ToolExecutionContext,
    project_id: &str,
    board_id: &str,
) -> Result<(), String> {
    let repos = ctx
        .repos
        .as_ref()
        .ok_or("workspace_board: no repositories available")?;
    let columns = repos
        .project_board_columns()
        .list(project_id, Some(board_id.to_string()))
        .await?;
    for column in &columns {
        spawn_cloud_upsert_column(ctx, column);
    }
    Ok(())
}

fn spawn_cloud_delete(ctx: &ToolExecutionContext, table: &'static str, id: &str) {
    if let Some(client) = ctx.cloud_client.clone() {
        let id = id.to_string();
        tokio::spawn(async move {
            if let Err(e) = client.delete_by_id(table, &id).await {
                tracing::warn!("cloud delete {} (tool): {}", table, e);
            }
        });
    }
}
