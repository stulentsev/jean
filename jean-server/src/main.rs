mod llm;

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use jean_shared::{ClientChatRequest, ClientMessage, ChatMessage, MessageRole, ChatResponse, StreamChunk, ToolCall};
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
    Json(request): Json<ClientChatRequest>,
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
        match chunk {
            StreamChunk::Text { delta, done } => {
                full_response.push_str(&delta);
                if done {
                    break;
                }
            }
            _ => {}
        }
    }

    Ok(Json(ChatResponse {
        content: full_response,
        model: llm_service.model().to_string(),
    }))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    llm_service: Arc<LlmService>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, llm_service))
}

async fn handle_socket(mut socket: WebSocket, llm_service: Arc<LlmService>) {
    info!("=== NEW WEBSOCKET CONNECTION ESTABLISHED ===");

    // Store conversation history for this connection
    let mut conversation_history: Vec<ChatMessage> = Vec::new();
    // Track pending tool calls from the assistant (for future use)
    let mut _pending_tool_calls: Vec<ToolCall> = Vec::new();

    while let Some(msg) = socket.recv().await {
        if let Ok(Message::Text(text)) = msg {
            info!("=== MESSAGE RECEIVED FROM CLIENT ===");
            info!("Raw message:\n{}", text);

            match serde_json::from_str::<ClientMessage>(&text) {
                Ok(ClientMessage::ChatRequest(request)) => {
                    info!("Message type: ChatRequest");
                    info!("Number of messages: {}", request.messages.len());
                    for (i, msg) in request.messages.iter().enumerate() {
                        info!("  Message {}: {:?} - {} chars", i, msg.role, msg.content.len());
                    }

                    // Update conversation history with new messages
                    conversation_history = request.messages.clone();

                    match llm_service.stream_chat(request.messages).await {
                        Ok(mut rx) => {
                            let mut assistant_response = String::new();
                            let mut current_tool_calls = Vec::new();

                            while let Some(chunk) = rx.recv().await {
                                let is_done = matches!(&chunk, StreamChunk::Text { done: true, .. });

                                // Log different types of chunks
                                match &chunk {
                                    StreamChunk::Text { delta, done } => {
                                        assistant_response.push_str(delta);
                                        if *done {
                                            info!("Sending completion chunk to client");
                                        }
                                    }
                                    StreamChunk::ToolCall { id, name, arguments } => {
                                        info!("=== SENDING TOOL CALL TO CLIENT ===");
                                        info!("Tool: {} (ID: {})", name, id);
                                        info!("Arguments: {}", arguments);
                                        // Store tool calls to track in conversation
                                        current_tool_calls.push(ToolCall {
                                            id: id.clone(),
                                            name: name.clone(),
                                            arguments: arguments.clone(),
                                        });
                                    }
                                    StreamChunk::ToolResult { id, content } => {
                                        info!("Sending tool result: {} - {}", id, content);
                                    }
                                }

                                if let Ok(response) = serde_json::to_string(&chunk) {
                                    if matches!(&chunk, StreamChunk::ToolCall { .. }) {
                                        info!("Serialized tool call message:\n{}", response);
                                    }
                                    if let Err(e) = socket.send(Message::Text(response)).await {
                                        error!("Failed to send chunk: {}", e);
                                        break;
                                    }
                                }
                                if is_done {
                                    // Handle the assistant's response based on what was received
                                    if !current_tool_calls.is_empty() {
                                        // Assistant made tool calls
                                        _pending_tool_calls = current_tool_calls.clone();
                                        conversation_history.push(ChatMessage {
                                            role: MessageRole::Assistant,
                                            content: String::new(), // Tool calls don't have text content
                                            tool_call_id: None,
                                            tool_calls: Some(current_tool_calls),
                                        });
                                    } else if !assistant_response.is_empty() {
                                        // Assistant provided a text response
                                        conversation_history.push(ChatMessage {
                                            role: MessageRole::Assistant,
                                            content: assistant_response.clone(),
                                            tool_call_id: None,
                                            tool_calls: None,
                                        });
                                    }
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to stream chat: {:?}", e);
                            let error_chunk = StreamChunk::Text {
                                delta: format!("Error: {}", e),
                                done: true,
                            };
                            if let Ok(response) = serde_json::to_string(&error_chunk) {
                                let _ = socket.send(Message::Text(response)).await;
                            }
                        }
                    }
                }
                Ok(ClientMessage::ToolResult { id, content }) => {
                    info!("=== TOOL RESULT RECEIVED FROM CLIENT ===");
                    info!("Tool ID: {}", id);
                    info!("Result content length: {} chars", content.len());
                    info!("Result preview (first 500 chars):\n{}",
                        if content.len() > 500 {
                            &content[..500]
                        } else {
                            &content
                        });

                    // Add tool result as a Tool message with proper tool_call_id
                    conversation_history.push(ChatMessage {
                        role: MessageRole::Tool,
                        content,
                        tool_call_id: Some(id.clone()),
                        tool_calls: None,
                    });

                    // Continue the conversation with the LLM
                    info!("Continuing conversation with tool result");
                    info!("Current conversation history length: {}", conversation_history.len());

                    // Log the conversation history for debugging
                    for (i, msg) in conversation_history.iter().enumerate() {
                        info!("  History[{}]: {:?} - {} chars", i, msg.role, msg.content.len());
                        if let Some(ref tool_calls) = msg.tool_calls {
                            for tc in tool_calls {
                                info!("    Tool call: {} ({})", tc.name, tc.id);
                            }
                        }
                        if let Some(ref tool_id) = msg.tool_call_id {
                            info!("    Tool result for: {}", tool_id);
                        }
                    }

                    match llm_service.stream_chat(conversation_history.clone()).await {
                        Ok(mut rx) => {
                            let mut assistant_response = String::new();
                            let mut current_tool_calls = Vec::new();

                            while let Some(chunk) = rx.recv().await {
                                let is_done = matches!(&chunk, StreamChunk::Text { done: true, .. });

                                match &chunk {
                                    StreamChunk::Text { delta, done } => {
                                        assistant_response.push_str(delta);
                                        if *done {
                                            info!("=== FINAL LLM RESPONSE AFTER TOOL CALL ===");
                                            info!("{}", assistant_response);
                                            info!("=== END RESPONSE ({} chars) ===", assistant_response.len());
                                        }
                                    }
                                    StreamChunk::ToolCall { id, name, arguments } => {
                                        info!("=== SENDING ANOTHER TOOL CALL TO CLIENT ===");
                                        info!("Tool: {} (ID: {})", name, id);
                                        info!("Arguments: {}", arguments);
                                        current_tool_calls.push(ToolCall {
                                            id: id.clone(),
                                            name: name.clone(),
                                            arguments: arguments.clone(),
                                        });
                                    }
                                    _ => {}
                                }

                                if let Ok(response) = serde_json::to_string(&chunk) {
                                    if let Err(e) = socket.send(Message::Text(response)).await {
                                        error!("Failed to send chunk: {}", e);
                                        break;
                                    }
                                } else {
                                    error!("Failed to serialize chunk");
                                }

                                if is_done {
                                    // Handle the assistant's response based on what was received
                                    if !current_tool_calls.is_empty() {
                                        // Assistant made more tool calls
                                        _pending_tool_calls = current_tool_calls.clone();
                                        conversation_history.push(ChatMessage {
                                            role: MessageRole::Assistant,
                                            content: String::new(),
                                            tool_call_id: None,
                                            tool_calls: Some(current_tool_calls),
                                        });
                                    } else if !assistant_response.is_empty() {
                                        // Assistant provided a text response
                                        conversation_history.push(ChatMessage {
                                            role: MessageRole::Assistant,
                                            content: assistant_response,
                                            tool_call_id: None,
                                            tool_calls: None,
                                        });
                                    }
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to continue chat after tool result: {:?}", e);
                            let error_chunk = StreamChunk::Text {
                                delta: format!("Error continuing conversation: {}", e),
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
                    let error_chunk = StreamChunk::Text {
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