use std::convert::Infallible;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::State,
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use uuid::Uuid;

use tracing::info;

use crate::engine::{ChatMessage, EngineHandle, InferResponse};
use crate::hardware::HardwareProfile;

#[derive(Clone)]
pub struct AppState {
    pub engine: EngineHandle,
    pub hardware: HardwareProfile,
}

#[derive(Debug, Deserialize)]
pub struct ChatCompletionRequest {
    #[serde(default)]
    pub model: Option<String>,
    pub messages: Vec<ChatMessage>,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_top_p")]
    pub top_p: f32,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub stream_options: Option<serde_json::Value>,
    #[serde(default)]
    pub tools: Option<serde_json::Value>,
    #[serde(default)]
    pub tool_choice: Option<serde_json::Value>,
}

fn default_temperature() -> f32 {
    0.7
}
fn default_top_p() -> f32 {
    0.95
}
fn default_max_tokens() -> usize {
    2048
}

pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // OpenAI Compatible Endpoints
        .route("/v1/models", get(list_models))
        .route("/v1/chat/completions", post(chat_completions))
        // Ollama Compatible Endpoints (for instant drop-in detection)
        .route("/api/tags", get(ollama_tags))
        .route("/api/chat", post(ollama_chat))
        .route("/api/generate", post(ollama_chat))
        // System & Telemetry Endpoints
        .route("/health", get(health_check))
        .route("/system/hardware", get(system_hardware))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health_check(State(state): State<AppState>) -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "service": "ClawMind AI Engine",
        "version": env!("CARGO_PKG_VERSION"),
        "engine_status": state.engine.status()
    }))
}

async fn system_hardware(State(state): State<AppState>) -> impl IntoResponse {
    Json(json!({
        "hardware": state.hardware,
        "engine_config": state.engine.config,
        "active_model": state.engine.model_name,
        "engine_status": state.engine.status()
    }))
}

async fn list_models(State(state): State<AppState>) -> impl IntoResponse {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    Json(json!({
        "object": "list",
        "data": [
            {
                "id": state.engine.model_name,
                "object": "model",
                "created": now,
                "owned_by": "clawmind"
            }
        ]
    }))
}

async fn ollama_tags(State(state): State<AppState>) -> impl IntoResponse {
    let model_tag = format!("{}:latest", state.engine.model_name);
    Json(json!({
        "models": [
            {
                "name": model_tag,
                "model": model_tag,
                "modified_at": "2026-09-04T00:00:00Z",
                "size": 2284584960i64,
                "details": {
                    "parent_model": "",
                    "format": "gguf",
                    "family": "gemma",
                    "families": ["gemma"],
                    "parameter_size": "2B",
                    "quantization_level": "Q4_K_M"
                }
            }
        ]
    }))
}

async fn chat_completions(
    State(state): State<AppState>,
    Json(req): Json<ChatCompletionRequest>,
) -> Response {
    let msg_count = req.messages.len();
    let prompt = EngineHandle::format_messages_to_prompt(&req.messages);
    info!(
        "📥 Received chat completion request: {} messages, prompt length: {} chars, stream: {}",
        msg_count,
        prompt.len(),
        req.stream
    );
    let mut rx = state
        .engine
        .infer(prompt, req.max_tokens, req.temperature, req.top_p)
        .await;

    let req_id = format!("chatcmpl-{}", Uuid::new_v4().simple());
    let model_name = state.engine.model_name.clone();
    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if req.stream {
        // SSE streaming mode
        let stream = async_stream::stream! {
            // Initial chunk: declare assistant role (required by OpenAI SDK and OpenClaw)
            let role_chunk = json!({
                "id": req_id,
                "object": "chat.completion.chunk",
                "created": created,
                "model": model_name,
                "choices": [
                    {
                        "index": 0,
                        "delta": {
                            "role": "assistant"
                        },
                        "finish_reason": null
                    }
                ]
            });
            yield Ok::<_, Infallible>(Event::default().data(role_chunk.to_string()));

            while let Some(msg) = rx.recv().await {
                match msg {
                    InferResponse::Token(token) => {
                        let chunk = json!({
                            "id": req_id,
                            "object": "chat.completion.chunk",
                            "created": created,
                            "model": model_name,
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {
                                        "content": token
                                    },
                                    "finish_reason": null
                                }
                            ]
                        });
                        yield Ok::<_, Infallible>(Event::default().data(chunk.to_string()));
                    }
                    InferResponse::Done { .. } => {
                        let final_chunk = json!({
                            "id": req_id,
                            "object": "chat.completion.chunk",
                            "created": created,
                            "model": model_name,
                            "choices": [
                                {
                                    "index": 0,
                                    "delta": {},
                                    "finish_reason": "stop"
                                }
                            ]
                        });
                        yield Ok(Event::default().data(final_chunk.to_string()));
                        yield Ok(Event::default().data("[DONE]"));
                        break;
                    }
                    InferResponse::Error(err) => {
                        let err_chunk = json!({
                            "error": {
                                "message": err,
                                "type": "server_error"
                            }
                        });
                        yield Ok(Event::default().data(err_chunk.to_string()));
                        break;
                    }
                }
            }
        };

        Sse::new(stream)
            .keep_alive(KeepAlive::default())
            .into_response()
    } else {
        // Non-streaming JSON mode
        let mut full_text = String::new();
        let mut prompt_toks = 0;
        let mut comp_toks = 0;

        while let Some(msg) = rx.recv().await {
            match msg {
                InferResponse::Token(token) => full_text.push_str(&token),
                InferResponse::Done {
                    prompt_tokens,
                    completion_tokens,
                    ..
                } => {
                    prompt_toks = prompt_tokens;
                    comp_toks = completion_tokens;
                }
                InferResponse::Error(err) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": { "message": err } })),
                    )
                        .into_response();
                }
            }
        }

        Json(json!({
            "id": req_id,
            "object": "chat.completion",
            "created": created,
            "model": model_name,
            "choices": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": full_text
                    },
                    "finish_reason": "stop"
                }
            ],
            "usage": {
                "prompt_tokens": prompt_toks,
                "completion_tokens": comp_toks,
                "total_tokens": prompt_toks + comp_toks
            }
        }))
        .into_response()
    }
}

async fn ollama_chat(
    State(state): State<AppState>,
    Json(req): Json<ChatCompletionRequest>,
) -> Response {
    // Re-use standard completions handler for Ollama compatibility
    chat_completions(State(state), Json(req)).await
}
