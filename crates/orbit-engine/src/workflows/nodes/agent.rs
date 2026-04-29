use std::path::PathBuf;

use serde_json::json;
use tauri::Manager;

use crate::auth::{AuthMode, AuthState};
use crate::commands::users::ActiveUser;
use crate::db::cloud::CloudClientState;
use crate::executor::engine::{
    AgentSemaphores, ExecutorTx, SessionExecutionRegistry, UserQuestionRegistry,
};
use crate::executor::{agent_loop, workspace};
use crate::memory_service::MemoryServiceState;
use crate::models::task::AgentLoopConfig;
use crate::workflows::nodes::{NodeExecutionContext, NodeFailure, NodeOutcome};
use crate::workflows::template::{
    parse_agent_output, render_agent_prompt, render_optional_template,
};

pub(super) async fn execute(ctx: &NodeExecutionContext<'_>) -> Result<NodeOutcome, NodeFailure> {
    let agent_id = ctx
        .node
        .data
        .get("agentId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "agent.run requires data.agentId".to_string())?
        .to_string();
    let template = ctx
        .node
        .data
        .get("promptTemplate")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let context = render_optional_template(
        ctx.node
            .data
            .get("contextTemplate")
            .and_then(|v| v.as_str()),
        ctx.outputs,
    );
    let output_mode = ctx
        .node
        .data
        .get("outputMode")
        .and_then(|v| v.as_str())
        .unwrap_or("text");
    let max_iterations = ctx
        .node
        .data
        .get("maxIterations")
        .and_then(|v| v.as_u64())
        .map(|value| value.min(u32::MAX as u64) as u32);
    let max_total_tokens = ctx
        .node
        .data
        .get("maxTotalTokens")
        .and_then(|v| v.as_u64())
        .map(|value| value.min(u32::MAX as u64) as u32);
    let prompt = render_agent_prompt(&template, context.as_deref(), output_mode, ctx.outputs);

    let ws_config = workspace::load_agent_config(&agent_id).unwrap_or_default();
    if ws_config.provider.is_empty() {
        return Err(format!("agent {} has no provider configured", agent_id).into());
    }
    let app = ctx
        .app_handle()
        .ok_or_else(|| "agent.run requires a Tauri runtime host".to_string())?;
    let runtime_app = app
        .try_state::<crate::RuntimeAppHandleState>()
        .map(|state| state.0.clone())
        .ok_or_else(|| "agent.run requires the managed runtime app handle".to_string())?;
    let executor_tx = runtime_app.state::<ExecutorTx>().0.clone();
    let agent_semaphores = runtime_app.state::<AgentSemaphores>().inner().clone();
    let session_registry = runtime_app
        .state::<SessionExecutionRegistry>()
        .inner()
        .clone();
    let user_question_registry = runtime_app.state::<UserQuestionRegistry>().inner().clone();
    let permission_registry =
        runtime_app.state::<crate::executor::permissions::PermissionRegistry>();
    let memory_client = ctx
        .app_handle()
        .ok_or_else(|| "agent.run requires a Tauri runtime host".to_string())?
        .state::<Option<MemoryServiceState>>()
        .as_ref()
        .map(|state| state.client.clone());
    let memory_user_id = resolve_memory_user_id(&runtime_app).await;
    let cloud_client = runtime_app.state::<CloudClientState>().get();

    let run_cfg = AgentLoopConfig {
        goal: prompt.clone(),
        model: None,
        max_iterations,
        max_total_tokens,
        template_vars: None,
    };
    let log_path = workflow_agent_log_path(&agent_id, ctx.workflow_id, &ctx.node.id);
    let outcome = agent_loop::run_agent_loop_for_workflow(
        ctx.run_id,
        &agent_id,
        &run_cfg,
        &log_path,
        &runtime_app,
        ctx.db,
        &executor_tx,
        Some(ctx.project_id),
        &agent_semaphores,
        &session_registry,
        &permission_registry,
        Some(&user_question_registry),
        memory_client.as_ref(),
        &memory_user_id,
        cloud_client,
    )
    .await
    .map_err(|e| format!("agent.run LLM loop failed: {}", e))?;

    let text = outcome.finish_summary.unwrap_or_default();
    let base_output = json!({
        "agentId": agent_id,
        "prompt": prompt,
        "context": context,
        "outputMode": output_mode,
        "iterations": outcome.iterations,
        "usage": {
            "inputTokens": outcome.input_tokens,
            "outputTokens": outcome.output_tokens,
            "totalTokens": outcome.input_tokens + outcome.output_tokens,
        },
        "text": text,
    });
    let parsed = match parse_agent_output(output_mode, &text) {
        Ok(value) => value,
        Err(err) => {
            let mut diagnostic = base_output.clone();
            if let Some(obj) = diagnostic.as_object_mut() {
                obj.insert("parsed".to_string(), serde_json::Value::Null);
                obj.insert(
                    "parseError".to_string(),
                    serde_json::Value::String(err.clone()),
                );
            }
            return Err(NodeFailure::with_output(err, diagnostic));
        }
    };

    let mut output = base_output;
    if let Some(obj) = output.as_object_mut() {
        obj.insert("parsed".to_string(), parsed);
    }
    Ok(NodeOutcome {
        output,
        next_handle: None,
    })
}

async fn resolve_memory_user_id<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> String {
    match app.state::<AuthState>().get().await {
        AuthMode::Cloud(session) => session.user_id,
        _ => app.state::<ActiveUser>().get().await,
    }
}

fn workflow_agent_log_path(agent_id: &str, workflow_id: &str, node_id: &str) -> PathBuf {
    workspace::agent_dir(agent_id)
        .join("workflow_runs")
        .join(format!("{}-{}.log", workflow_id, node_id))
}
