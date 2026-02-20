use super::auth::validate_api_key;
use super::streaming::build_sse_response;
use super::types::{
    ChatCompletion, ChatCompletionRequest, Choice, ChoiceMessage, CompletionUsage, RequestMessage,
};
use crate::gateway::AppState;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json};

pub async fn handle_chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ChatCompletionRequest>,
) -> impl IntoResponse {
    let api_keys = state.openai_compat_api_keys.as_deref().unwrap_or(&[]);
    if !validate_api_key(&headers, api_keys) {
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

    match state
        .provider
        .chat_with_system_full(system_prompt.as_deref(), &user_message, &model, temperature)
        .await
    {
        Ok(response) => {
            let completion_id = format!("chatcmpl-{}", uuid::Uuid::new_v4());
            let created = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_secs());

            if request.stream.unwrap_or(false) {
                build_sse_response(&completion_id, &model, &response.text, created).into_response()
            } else {
                let usage = response.input_tokens.zip(response.output_tokens).map(
                    |(input_tokens, output_tokens)| CompletionUsage {
                        prompt_tokens: input_tokens,
                        completion_tokens: output_tokens,
                        total_tokens: input_tokens + output_tokens,
                    },
                );

                Json(ChatCompletion {
                    id: completion_id,
                    object: "chat.completion".to_string(),
                    created,
                    model,
                    choices: vec![Choice {
                        index: 0,
                        message: ChoiceMessage {
                            role: "assistant".to_string(),
                            content: response.text,
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
    use crate::gateway::openai_compat::types::RequestMessage;

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
