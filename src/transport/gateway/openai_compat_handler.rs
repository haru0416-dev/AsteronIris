use crate::agent::tool_loop::{LoopStopReason, ToolLoop, ToolLoopRunParams};
use crate::security::policy::TenantPolicyContext;
use crate::tools::ExecutionContext;
use crate::transport::gateway::AppState;
use crate::transport::gateway::openai_compat_auth::validate_api_key;
use crate::transport::gateway::openai_compat_streaming::build_sse_response;
use crate::transport::gateway::openai_compat_types::{
    ChatCompletion, ChatCompletionRequest, Choice, ChoiceMessage, CompletionUsage, RequestMessage,
};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json};
use std::sync::Arc;

#[allow(clippy::too_many_lines)]
pub async fn handle_chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ChatCompletionRequest>,
) -> impl IntoResponse {
    // Primary auth: pairing bearer token (if pairing is active)
    let pairing_ok = if state.pairing.is_paired() || state.pairing.require_pairing() {
        bearer_token(&headers).is_some_and(|token| state.pairing.is_authenticated(token))
    } else {
        false
    };

    // Secondary auth: OpenAI-compat API key check
    let api_key_ok = {
        let auth_disabled = state.openai_compat_api_keys.is_none();
        let api_keys = state.openai_compat_api_keys.as_deref().unwrap_or(&[]);
        validate_api_key(&headers, api_keys, auth_disabled)
    };

    if !pairing_ok && !api_key_ok {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": { "message": "Invalid API key", "type": "invalid_request_error" }
            })),
        )
            .into_response();
    }

    let (system_prompt, user_message) = extract_messages(&request.messages);
    let temperature = request.temperature.unwrap_or(state.temperature);
    let model = request.model;
    let source_identifier = bearer_token(&headers).unwrap_or("openai-compat");
    let tool_loop = ToolLoop::new(Arc::clone(&state.registry), state.max_tool_loop_iterations);
    let ctx = ExecutionContext {
        security: Arc::clone(&state.security),
        autonomy_level: state.security.autonomy,
        entity_id: format!("gateway:{source_identifier}"),
        turn_number: 0,
        workspace_dir: state.security.workspace_dir.clone(),
        allowed_tools: None,
        rate_limiter: Arc::clone(&state.rate_limiter),
        tenant_context: TenantPolicyContext::disabled(),
    };

    match tool_loop
        .run(ToolLoopRunParams {
            provider: state.provider.as_ref(),
            system_prompt: system_prompt.as_deref().unwrap_or_default(),
            user_message: &user_message,
            image_content: &[],
            model: &model,
            temperature,
            ctx: &ctx,
            stream_sink: None,
            conversation_history: &[],
            hooks: &[],
        })
        .await
    {
        Ok(result) => {
            if let LoopStopReason::Error(error) = &result.stop_reason {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": { "message": error, "type": "server_error" }
                    })),
                )
                    .into_response();
            }
            if matches!(result.stop_reason, LoopStopReason::MaxIterations) {
                tracing::warn!("openai compat tool loop hit max iterations");
            }
            let completion_id = format!("chatcmpl-{}", uuid::Uuid::new_v4());
            let created = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_secs());

            if request.stream.unwrap_or(false) {
                build_sse_response(&completion_id, &model, &result.final_text, created)
                    .into_response()
            } else {
                let usage = result.tokens_used.map(|total_tokens| CompletionUsage {
                    prompt_tokens: 0,
                    completion_tokens: total_tokens,
                    total_tokens,
                });

                Json(ChatCompletion {
                    id: completion_id,
                    object: "chat.completion".to_string(),
                    created,
                    model,
                    choices: vec![Choice {
                        index: 0,
                        message: ChoiceMessage {
                            role: "assistant".to_string(),
                            content: result.final_text,
                        },
                        finish_reason: "stop".to_string(),
                    }],
                    usage,
                })
                .into_response()
            }
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": { "message": error.to_string(), "type": "server_error" }
            })),
        )
            .into_response(),
    }
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.strip_prefix("Bearer "))
        .filter(|token| !token.is_empty())
}

fn extract_messages(messages: &[RequestMessage]) -> (Option<String>, String) {
    let system = messages
        .iter()
        .filter(|message| message.role == "system")
        .map(|message| message.content.clone())
        .reduce(|acc, content| format!("{acc}\n{content}"));

    let user_message = messages
        .iter()
        .filter(|message| message.role != "system")
        .map(|message| format!("{}: {}", message.role, message.content))
        .collect::<Vec<_>>()
        .join("\n");

    (system, user_message)
}

#[cfg(test)]
mod tests {
    use super::extract_messages;
    use crate::transport::gateway::openai_compat_types::RequestMessage;

    #[test]
    fn extract_messages_with_system_and_user() {
        let messages = vec![
            RequestMessage {
                role: "system".to_string(),
                content: "You are concise.".to_string(),
            },
            RequestMessage {
                role: "user".to_string(),
                content: "Hello".to_string(),
            },
        ];

        let (system, user) = extract_messages(&messages);
        assert_eq!(system.as_deref(), Some("You are concise."));
        assert_eq!(user, "user: Hello");
    }

    #[test]
    fn extract_messages_with_only_user() {
        let messages = vec![RequestMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
        }];

        let (system, user) = extract_messages(&messages);
        assert!(system.is_none());
        assert_eq!(user, "user: Hello");
    }

    #[test]
    fn extract_messages_with_multiple_system_messages() {
        let messages = vec![
            RequestMessage {
                role: "system".to_string(),
                content: "Rule one.".to_string(),
            },
            RequestMessage {
                role: "system".to_string(),
                content: "Rule two.".to_string(),
            },
            RequestMessage {
                role: "user".to_string(),
                content: "Proceed".to_string(),
            },
        ];

        let (system, user) = extract_messages(&messages);
        assert_eq!(system.as_deref(), Some("Rule one.\nRule two."));
        assert_eq!(user, "user: Proceed");
    }
}
