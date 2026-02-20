use super::types::{ChatCompletionChunk, ChunkChoice, ChunkDelta};
use axum::body::Body;
use axum::http::{Response, StatusCode, header};

pub fn build_sse_response(
    completion_id: &str,
    model: &str,
    content: &str,
    created: u64,
) -> Response<Body> {
    let chunks = build_chunks(completion_id, model, content, created);
    let stream = async_stream::stream! {
        for chunk in chunks {
            if let Ok(json) = serde_json::to_string(&chunk) {
                yield Ok::<_, std::convert::Infallible>(format!("data: {json}\n\n"));
            }
        }
        yield Ok("data: [DONE]\n\n".to_string());
    };

    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("text/event-stream"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-cache"),
    );
    response.headers_mut().insert(
        header::CONNECTION,
        header::HeaderValue::from_static("keep-alive"),
    );
    response
}

fn build_chunks(id: &str, model: &str, content: &str, created: u64) -> Vec<ChatCompletionChunk> {
    let mut chunks = vec![ChatCompletionChunk {
        id: id.to_string(),
        object: "chat.completion.chunk".to_string(),
        created,
        model: model.to_string(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: Some("assistant".to_string()),
                content: None,
            },
            finish_reason: None,
        }],
    }];

    for paragraph in content.split("\n\n") {
        chunks.push(ChatCompletionChunk {
            id: id.to_string(),
            object: "chat.completion.chunk".to_string(),
            created,
            model: model.to_string(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta {
                    role: None,
                    content: Some(paragraph.to_string()),
                },
                finish_reason: None,
            }],
        });
    }

    chunks.push(ChatCompletionChunk {
        id: id.to_string(),
        object: "chat.completion.chunk".to_string(),
        created,
        model: model.to_string(),
        choices: vec![ChunkChoice {
            index: 0,
            delta: ChunkDelta {
                role: None,
                content: None,
            },
            finish_reason: Some("stop".to_string()),
        }],
    });

    chunks
}
