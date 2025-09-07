mod llm;

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use speak_code_shared::{ChatRequest, ChatResponse, StreamChunk};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::{info, error};
use llm::LlmService;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    dotenv::dotenv().ok();

    let api_key = match std::env::var("OPENAI_API_KEY") {
        Ok(key) if key.starts_with("sk-") => {
            info!("OpenAI API key loaded successfully");
            key
        }
        Ok(_) => {
            error!("OPENAI_API_KEY found but doesn't start with 'sk-'. Please check your .env file");
            panic!("Invalid OpenAI API key format");
        }
        Err(_) => {
            error!("OPENAI_API_KEY not found. Please set it in your .env file");
            panic!("OPENAI_API_KEY must be set in .env file");
        }
    };
    
    let model = std::env::var("OPENAI_MODEL")
        .expect("OPENAI_MODEL must be set in .env file");
    
    info!("Using OpenAI model: {}", model);
    let llm_service = Arc::new(LlmService::new(api_key, model.clone()));

    let app = Router::new()
        .route("/health", get(health))
        .route("/chat", post({
            let llm = llm_service.clone();
            move |req| chat(req, llm)
        }))
        .route("/ws/chat", get({
            let llm = llm_service.clone();
            move |ws| ws_handler(ws, llm)
        }))
        .layer(CorsLayer::permissive());

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    info!("Server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health() -> &'static str {
    "OK"
}

async fn chat(
    Json(request): Json<ChatRequest>,
    llm_service: Arc<LlmService>,
) -> Result<Json<ChatResponse>, StatusCode> {
    let mut rx = llm_service
        .stream_chat(request.messages.clone())
        .await
        .map_err(|e| {
            error!("Failed to stream chat: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let mut full_response = String::new();
    while let Some(chunk) = rx.recv().await {
        full_response.push_str(&chunk.delta);
        if chunk.done {
            break;
        }
    }

    Ok(Json(ChatResponse {
        content: full_response,
        model: request.model,
    }))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    llm_service: Arc<LlmService>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, llm_service))
}

async fn handle_socket(mut socket: WebSocket, llm_service: Arc<LlmService>) {
    while let Some(msg) = socket.recv().await {
        if let Ok(Message::Text(text)) = msg {
            match serde_json::from_str::<ChatRequest>(&text) {
                Ok(request) => {
                    match llm_service.stream_chat(request.messages).await {
                        Ok(mut rx) => {
                            while let Some(chunk) = rx.recv().await {
                                let is_done = chunk.done;
                                if let Ok(response) = serde_json::to_string(&chunk) {
                                    if let Err(e) = socket.send(Message::Text(response)).await {
                                        error!("Failed to send chunk: {}", e);
                                        break;
                                    }
                                }
                                if is_done {
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to stream chat: {:?}", e);
                            let error_chunk = StreamChunk {
                                delta: format!("Error: {}", e),
                                done: true,
                            };
                            if let Ok(response) = serde_json::to_string(&error_chunk) {
                                let _ = socket.send(Message::Text(response)).await;
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to parse request: {}", e);
                    let error_chunk = StreamChunk {
                        delta: format!("Invalid request format: {}", e),
                        done: true,
                    };
                    if let Ok(response) = serde_json::to_string(&error_chunk) {
                        let _ = socket.send(Message::Text(response)).await;
                    }
                }
            }
        }
    }
}