use serde_json::json;

use crate::executor::{keychain, llm_provider, workspace};
use crate::workflows::nodes::{NodeExecutionContext, NodeOutcome};
use crate::workflows::template::{
    parse_agent_output, render_agent_prompt, render_optional_template,
};

pub(super) async fn execute(ctx: &NodeExecutionContext<'_>) -> Result<NodeOutcome, String> {
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
    let prompt = render_agent_prompt(&template, context.as_deref(), output_mode, ctx.outputs);

    let ws_config = workspace::load_agent_config(&agent_id).unwrap_or_default();
    if ws_config.provider.is_empty() {
        return Err(format!("agent {} has no provider configured", agent_id));
    }
    let api_key = keychain::retrieve_api_key(&ws_config.provider).map_err(|_| {
        format!(
            "no API key configured for provider `{}`",
            ws_config.provider
        )
    })?;
    let provider = llm_provider::create_provider(&ws_config.provider, api_key)?;

    let llm_config = llm_provider::LlmConfig {
        model: ws_config.model.clone(),
        max_tokens: 4_096,
        temperature: Some(ws_config.temperature),
        system_prompt: ws_config
            .role_system_instructions
            .clone()
            .unwrap_or_default(),
    };
    let messages = vec![llm_provider::ChatMessage {
        role: "user".to_string(),
        content: vec![llm_provider::ContentBlock::Text {
            text: prompt.clone(),
        }],
        created_at: None,
    }];

    let response = provider
        .chat_complete(&llm_config, &messages, &[])
        .await
        .map_err(|e| format!("agent.run LLM call failed: {}", e))?;

    let text = llm_provider::extract_text_response(&response).unwrap_or_default();
    let parsed = parse_agent_output(output_mode, &text)?;

    Ok(NodeOutcome {
        output: json!({
            "agentId": agent_id,
            "prompt": prompt,
            "context": context,
            "outputMode": output_mode,
            "text": text,
            "parsed": parsed,
        }),
        next_handle: None,
    })
}
