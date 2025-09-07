use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use speak_code_shared::{ChatRequest, ChatResponse, StreamChunk};
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let app = Router::new()
        .route("/health", get(health))
        .route("/chat", post(chat))
        .route("/ws/chat", get(ws_handler))
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

async fn chat(Json(request): Json<ChatRequest>) -> Result<Json<ChatResponse>, StatusCode> {
    // TODO: Implement OpenAI/Anthropic API call
    Ok(Json(ChatResponse {
        content: "Hello from Rust backend!".to_string(),
        model: request.model,
    }))
}

async fn ws_handler(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(mut socket: WebSocket) {
    while let Some(msg) = socket.recv().await {
        if let Ok(msg) = msg {
            if let Message::Text(text) = msg {
                if let Ok(request) = serde_json::from_str::<ChatRequest>(&text) {
                    // TODO: Stream OpenAI/Anthropic response
                    let chunk = StreamChunk {
                        delta: "Streaming from Rust!".to_string(),
                        done: true,
                    };
                    
                    if let Ok(response) = serde_json::to_string(&chunk) {
                        let _ = socket.send(Message::Text(response)).await;
                    }
                }
            }
        }
    }
}