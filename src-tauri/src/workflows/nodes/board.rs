use serde_json::{json, Value};
use tauri::Manager;

use crate::commands::work_items::{
    block_work_item_with_db, claim_work_item_with_db, complete_work_item_with_db,
    create_work_item_comment_with_db, create_work_item_with_db, delete_work_item_with_db,
    get_work_item_with_db, list_work_item_comments_with_db, list_work_items_with_db,
    move_work_item_with_db, update_work_item_with_db,
};
use crate::db::cloud::CloudClientState;
use crate::models::work_item::{CreateWorkItem, UpdateWorkItem};
use crate::models::work_item_comment::{CommentAuthor, WorkItemComment};
use crate::workflows::nodes::{NodeExecutionContext, NodeFailure, NodeOutcome};
use crate::workflows::template::{
    json_number_to_i64, lookup_json_path, optional_labels, parse_optional_priority,
    parse_optional_work_item_kind, parse_optional_work_item_status, parse_priority,
    parse_work_item_kind, parse_work_item_labels, render_optional_template, render_required_field,
    render_template, required_template,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkItemAction {
    Create,
    List,
    Get,
    Update,
    Move,
    Block,
    Complete,
    Comment,
    ListComments,
    Delete,
    Claim,
}

impl WorkItemAction {
    fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.unwrap_or("create") {
            "create" => Ok(Self::Create),
            "list" => Ok(Self::List),
            "get" => Ok(Self::Get),
            "update" => Ok(Self::Update),
            "move" => Ok(Self::Move),
            "block" => Ok(Self::Block),
            "complete" => Ok(Self::Complete),
            "comment" => Ok(Self::Comment),
            "list_comments" => Ok(Self::ListComments),
            "delete" => Ok(Self::Delete),
            "claim" => Ok(Self::Claim),
            other => Err(format!(
                "board.work_item has unsupported action '{}'",
                other
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::List => "list",
            Self::Get => "get",
            Self::Update => "update",
            Self::Move => "move",
            Self::Block => "block",
            Self::Complete => "complete",
            Self::Comment => "comment",
            Self::ListComments => "list_comments",
            Self::Delete => "delete",
            Self::Claim => "claim",
        }
    }
}

pub(super) async fn execute_proposal_enqueue<R: tauri::Runtime>(
    ctx: &NodeExecutionContext<'_, R>,
) -> Result<NodeOutcome, NodeFailure> {
    let candidates_path =
        required_template(&ctx.node.data, "candidatesPath", "board.proposal.enqueue")?;
    let review_column_id =
        required_template(&ctx.node.data, "reviewColumnId", "board.proposal.enqueue")?;
    let kind = parse_work_item_kind(ctx.node.data.get("kind").and_then(|v| v.as_str()))?;
    let priority = parse_priority(ctx.node.data.get("priority")).clamp(0, 3);
    let labels = parse_work_item_labels(
        ctx.node.data.get("labelsText").and_then(|v| v.as_str()),
        ctx.outputs,
    );
    let candidates = lookup_json_path(&candidates_path, ctx.outputs)
        .and_then(|value| value.as_array().cloned())
        .ok_or_else(|| {
            format!(
                "board.proposal.enqueue requires candidatesPath '{}' to resolve to an array",
                candidates_path
            )
        })?;

    let mut work_items = Vec::new();
    for candidate in candidates {
        if !candidate
            .get("shouldReview")
            .and_then(Value::as_bool)
            .unwrap_or(true)
        {
            continue;
        }

        let listing = candidate.get("listing").cloned().unwrap_or(Value::Null);
        let title = listing
            .get("title")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| "Freelance job proposal review".to_string());
        let proposal_draft = candidate
            .get("proposalDraft")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if proposal_draft.trim().is_empty() {
            return Err("board.proposal.enqueue candidate is missing proposalDraft".into());
        }
        let fit_score = candidate.get("fitScore").cloned().unwrap_or(Value::Null);
        let fit_reason = candidate
            .get("fitReason")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        let description = Some(format!(
            "Proposal draft:\n\n{}\n\nFit reason:\n{}\n",
            proposal_draft,
            if fit_reason.is_empty() {
                "(not provided)"
            } else {
                fit_reason.as_str()
            }
        ));

        let payload = CreateWorkItem {
            project_id: ctx.project_id.to_string(),
            board_id: None,
            title,
            description,
            kind: Some(kind.clone()),
            column_id: Some(review_column_id.clone()),
            status: None,
            priority: Some(priority),
            assignee_agent_id: None,
            created_by_agent_id: None,
            parent_work_item_id: None,
            position: None,
            labels: Some(labels.clone()),
            metadata: Some(json!({
                "source": "workflow.proposal",
                "workflowRunId": ctx.run_id,
                "workflowNodeId": ctx.node.id,
                "proposalCandidate": candidate,
                "listing": listing,
                "fitScore": fit_score,
            })),
        };
        let item = create_work_item_with_db(ctx.db, payload).await?;
        sync_work_item_cloud(ctx.app, item.clone());
        work_items.push(item);
    }

    Ok(NodeOutcome {
        output: json!({
            "action": "enqueue",
            "reviewColumnId": review_column_id,
            "count": work_items.len(),
            "workItems": work_items,
        }),
        next_handle: None,
    })
}

pub(super) async fn execute_work_item<R: tauri::Runtime>(
    ctx: &NodeExecutionContext<'_, R>,
) -> Result<NodeOutcome, NodeFailure> {
    let action = WorkItemAction::parse(ctx.node.data.get("action").and_then(|v| v.as_str()))?;

    match action {
        WorkItemAction::Create => {
            let title_template =
                required_template(&ctx.node.data, "titleTemplate", action.as_str())?;
            let title = render_template(&title_template, ctx.outputs)
                .trim()
                .to_string();
            if title.is_empty() {
                return Err("board.work_item rendered an empty title".into());
            }

            let description = render_optional_template(
                ctx.node
                    .data
                    .get("descriptionTemplate")
                    .and_then(|v| v.as_str()),
                ctx.outputs,
            );
            let assignee_agent_id = render_optional_template(
                ctx.node
                    .data
                    .get("assigneeAgentId")
                    .and_then(|v| v.as_str()),
                ctx.outputs,
            );
            let parent_work_item_id = render_optional_template(
                ctx.node
                    .data
                    .get("parentWorkItemId")
                    .and_then(|v| v.as_str()),
                ctx.outputs,
            );
            let labels = parse_work_item_labels(
                ctx.node.data.get("labelsText").and_then(|v| v.as_str()),
                ctx.outputs,
            );
            let kind = parse_work_item_kind(ctx.node.data.get("kind").and_then(|v| v.as_str()))?;
            let status = parse_optional_work_item_status(
                ctx.node.data.get("status").and_then(|v| v.as_str()),
            )?;
            let column_id = render_optional_template(
                ctx.node.data.get("columnId").and_then(|v| v.as_str()),
                ctx.outputs,
            );
            let priority = parse_priority(ctx.node.data.get("priority")).clamp(0, 3);

            let payload = CreateWorkItem {
                project_id: ctx.project_id.to_string(),
                board_id: None,
                title: title.clone(),
                description: description.clone(),
                kind: Some(kind.clone()),
                column_id: column_id.clone(),
                status: status.clone(),
                priority: Some(priority),
                assignee_agent_id: assignee_agent_id.clone(),
                created_by_agent_id: None,
                parent_work_item_id: parent_work_item_id.clone(),
                position: None,
                labels: Some(labels.clone()),
                metadata: Some(json!({
                    "source": "workflow",
                    "workflowRunId": ctx.run_id,
                    "workflowNodeId": ctx.node.id,
                })),
            };

            let item = create_work_item_with_db(ctx.db, payload).await?;
            sync_work_item_cloud(ctx.app, item.clone());

            Ok(NodeOutcome {
                output: json!({
                    "action": action.as_str(),
                    "title": title,
                    "description": description,
                    "kind": kind,
                    "columnId": column_id,
                    "status": item.status,
                    "priority": priority,
                    "labels": labels,
                    "assigneeAgentId": assignee_agent_id,
                    "parentWorkItemId": parent_work_item_id,
                    "workItem": item,
                }),
                next_handle: None,
            })
        }
        WorkItemAction::List => {
            let mut items =
                list_work_items_with_db(ctx.db, ctx.project_id.to_string(), None).await?;
            let column_id_filter = render_optional_template(
                ctx.node.data.get("listColumnId").and_then(|v| v.as_str()),
                ctx.outputs,
            );
            let status_filter = parse_optional_work_item_status(
                ctx.node.data.get("listStatus").and_then(|v| v.as_str()),
            )?;
            let kind_filter = parse_optional_work_item_kind(
                ctx.node.data.get("listKind").and_then(|v| v.as_str()),
            )?;
            let assignee_filter = render_optional_template(
                ctx.node.data.get("listAssignee").and_then(|v| v.as_str()),
                ctx.outputs,
            );
            let limit = ctx
                .node
                .data
                .get("limit")
                .and_then(json_number_to_i64)
                .filter(|v| *v > 0)
                .unwrap_or(100) as usize;

            if let Some(column_id) = column_id_filter.as_ref() {
                items.retain(|item| item.column_id.as_deref() == Some(column_id.as_str()));
            }
            if let Some(status) = status_filter.as_ref() {
                items.retain(|item| item.status == status.as_str());
            }
            if let Some(kind) = kind_filter.as_ref() {
                items.retain(|item| item.kind == kind.as_str());
            }
            if let Some(assignee) = assignee_filter.clone() {
                match assignee.as_str() {
                    "none" | "unassigned" | "null" => {
                        items.retain(|item| item.assignee_agent_id.is_none());
                    }
                    _ => items.retain(|item| {
                        item.assignee_agent_id.as_deref() == Some(assignee.as_str())
                    }),
                }
            }
            if items.len() > limit {
                items.truncate(limit);
            }

            Ok(NodeOutcome {
                output: json!({
                    "action": action.as_str(),
                    "count": items.len(),
                    "items": items,
                    "filters": {
                        "column": status_filter.clone(),
                        "columnId": column_id_filter,
                        "status": status_filter,
                        "kind": kind_filter,
                        "assignee": assignee_filter,
                        "limit": limit,
                    },
                }),
                next_handle: None,
            })
        }
        WorkItemAction::Get => {
            let item_id = render_required_field(
                &ctx.node.data,
                "itemIdTemplate",
                action.as_str(),
                ctx.outputs,
            )?;
            let item = get_work_item_with_db(ctx.db, item_id.clone()).await?;
            Ok(NodeOutcome {
                output: json!({
                    "action": action.as_str(),
                    "itemId": item_id,
                    "workItem": item,
                }),
                next_handle: None,
            })
        }
        WorkItemAction::Update => {
            let item_id = render_required_field(
                &ctx.node.data,
                "itemIdTemplate",
                action.as_str(),
                ctx.outputs,
            )?;
            let kind =
                parse_optional_work_item_kind(ctx.node.data.get("kind").and_then(|v| v.as_str()))?;
            let priority = parse_optional_priority(ctx.node.data.get("priority"));
            let labels = optional_labels(
                ctx.node.data.get("labelsText").and_then(|v| v.as_str()),
                ctx.outputs,
            );
            let item = update_work_item_with_db(
                ctx.db,
                item_id.clone(),
                UpdateWorkItem {
                    title: render_optional_template(
                        ctx.node.data.get("titleTemplate").and_then(|v| v.as_str()),
                        ctx.outputs,
                    ),
                    description: render_optional_template(
                        ctx.node
                            .data
                            .get("descriptionTemplate")
                            .and_then(|v| v.as_str()),
                        ctx.outputs,
                    ),
                    kind,
                    column_id: render_optional_template(
                        ctx.node.data.get("columnId").and_then(|v| v.as_str()),
                        ctx.outputs,
                    ),
                    priority,
                    labels,
                    metadata: None,
                },
            )
            .await?;
            sync_work_item_cloud(ctx.app, item.clone());
            Ok(NodeOutcome {
                output: json!({
                    "action": action.as_str(),
                    "itemId": item_id,
                    "workItem": item,
                }),
                next_handle: None,
            })
        }
        WorkItemAction::Move => {
            let item_id = render_required_field(
                &ctx.node.data,
                "itemIdTemplate",
                action.as_str(),
                ctx.outputs,
            )?;
            let column_id = render_optional_template(
                ctx.node.data.get("columnId").and_then(|v| v.as_str()),
                ctx.outputs,
            );
            if column_id.is_none() {
                return Err(
                    "board.work_item move requires data.columnId; status-based moves are no longer supported"
                        .into(),
                );
            }
            let item =
                move_work_item_with_db(ctx.db, item_id.clone(), column_id.clone(), None).await?;
            sync_work_item_cloud(ctx.app, item.clone());
            Ok(NodeOutcome {
                output: json!({
                    "action": action.as_str(),
                    "itemId": item_id,
                    "columnId": column_id,
                    "status": item.status,
                    "workItem": item,
                }),
                next_handle: None,
            })
        }
        WorkItemAction::Block => {
            let item_id = render_required_field(
                &ctx.node.data,
                "itemIdTemplate",
                action.as_str(),
                ctx.outputs,
            )?;
            let reason = render_required_field(
                &ctx.node.data,
                "reasonTemplate",
                action.as_str(),
                ctx.outputs,
            )?;
            let item = block_work_item_with_db(ctx.db, item_id.clone(), reason.clone()).await?;
            sync_work_item_cloud(ctx.app, item.clone());
            Ok(NodeOutcome {
                output: json!({
                    "action": action.as_str(),
                    "itemId": item_id,
                    "reason": reason,
                    "workItem": item,
                }),
                next_handle: None,
            })
        }
        WorkItemAction::Complete => {
            let item_id = render_required_field(
                &ctx.node.data,
                "itemIdTemplate",
                action.as_str(),
                ctx.outputs,
            )?;
            let item = complete_work_item_with_db(ctx.db, item_id.clone()).await?;
            sync_work_item_cloud(ctx.app, item.clone());
            Ok(NodeOutcome {
                output: json!({
                    "action": action.as_str(),
                    "itemId": item_id,
                    "workItem": item,
                }),
                next_handle: None,
            })
        }
        WorkItemAction::Comment => {
            let item_id = render_required_field(
                &ctx.node.data,
                "itemIdTemplate",
                action.as_str(),
                ctx.outputs,
            )?;
            let body = render_required_field(
                &ctx.node.data,
                "bodyTemplate",
                action.as_str(),
                ctx.outputs,
            )?;
            let author = match render_optional_template(
                ctx.node
                    .data
                    .get("commentAuthorAgentId")
                    .and_then(|v| v.as_str()),
                ctx.outputs,
            ) {
                Some(agent_id) => CommentAuthor::Agent { agent_id },
                None => CommentAuthor::User,
            };
            let comment =
                create_work_item_comment_with_db(ctx.db, item_id.clone(), body.clone(), author)
                    .await?;
            sync_work_item_comment_cloud(ctx.app, comment.clone());
            Ok(NodeOutcome {
                output: json!({
                    "action": action.as_str(),
                    "itemId": item_id,
                    "body": body,
                    "comment": comment,
                }),
                next_handle: None,
            })
        }
        WorkItemAction::ListComments => {
            let item_id = render_required_field(
                &ctx.node.data,
                "itemIdTemplate",
                action.as_str(),
                ctx.outputs,
            )?;
            let comments = list_work_item_comments_with_db(ctx.db, item_id.clone()).await?;
            Ok(NodeOutcome {
                output: json!({
                    "action": action.as_str(),
                    "itemId": item_id,
                    "count": comments.len(),
                    "comments": comments,
                }),
                next_handle: None,
            })
        }
        WorkItemAction::Delete => {
            let item_id = render_required_field(
                &ctx.node.data,
                "itemIdTemplate",
                action.as_str(),
                ctx.outputs,
            )?;
            delete_work_item_with_db(ctx.db, item_id.clone()).await?;
            delete_work_item_cloud(ctx.app, item_id.clone());
            Ok(NodeOutcome {
                output: json!({
                    "action": action.as_str(),
                    "itemId": item_id,
                    "deleted": true,
                }),
                next_handle: None,
            })
        }
        WorkItemAction::Claim => {
            let item_id = render_required_field(
                &ctx.node.data,
                "itemIdTemplate",
                action.as_str(),
                ctx.outputs,
            )?;
            let agent_id = render_required_field(
                &ctx.node.data,
                "assigneeAgentId",
                action.as_str(),
                ctx.outputs,
            )?;
            let item = claim_work_item_with_db(ctx.db, item_id.clone(), agent_id.clone()).await?;
            sync_work_item_cloud(ctx.app, item.clone());
            Ok(NodeOutcome {
                output: json!({
                    "action": action.as_str(),
                    "itemId": item_id,
                    "agentId": agent_id,
                    "workItem": item,
                }),
                next_handle: None,
            })
        }
    }
}

fn sync_work_item_cloud<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    item: crate::models::work_item::WorkItem,
) {
    if let Some(client) = app.state::<CloudClientState>().get() {
        tokio::spawn(async move {
            if let Err(e) = client.upsert_work_item(&item).await {
                tracing::warn!("cloud upsert work_item (workflow): {}", e);
            }
        });
    }
}

fn sync_work_item_comment_cloud<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    comment: WorkItemComment,
) {
    if let Some(client) = app.state::<CloudClientState>().get() {
        tokio::spawn(async move {
            if let Err(e) = client.upsert_work_item_comment(&comment).await {
                tracing::warn!("cloud upsert work_item_comment (workflow): {}", e);
            }
        });
    }
}

fn delete_work_item_cloud<R: tauri::Runtime>(app: &tauri::AppHandle<R>, id: String) {
    if let Some(client) = app.state::<CloudClientState>().get() {
        tokio::spawn(async move {
            if let Err(e) = client.delete_by_id("work_items", &id).await {
                tracing::warn!("cloud delete work_item (workflow): {}", e);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::WorkItemAction;

    #[test]
    fn parses_all_supported_work_item_actions() {
        let cases = [
            (None, WorkItemAction::Create),
            (Some("create"), WorkItemAction::Create),
            (Some("list"), WorkItemAction::List),
            (Some("get"), WorkItemAction::Get),
            (Some("update"), WorkItemAction::Update),
            (Some("move"), WorkItemAction::Move),
            (Some("block"), WorkItemAction::Block),
            (Some("complete"), WorkItemAction::Complete),
            (Some("comment"), WorkItemAction::Comment),
            (Some("list_comments"), WorkItemAction::ListComments),
            (Some("delete"), WorkItemAction::Delete),
            (Some("claim"), WorkItemAction::Claim),
        ];

        for (input, expected) in cases {
            assert_eq!(WorkItemAction::parse(input).unwrap(), expected);
        }
    }

    #[test]
    fn rejects_unsupported_work_item_action() {
        assert!(WorkItemAction::parse(Some("archive")).is_err());
    }
}
