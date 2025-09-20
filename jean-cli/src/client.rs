use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use jean_shared::{ClientChatRequest, ClientMessage, StreamChunk};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{error, debug, warn, info};

#[derive(Clone)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

pub struct BackendClient {
    tx: mpsc::UnboundedSender<ClientMessage>,
}

impl BackendClient {
    pub fn new(ws_url: String) -> (Self, mpsc::UnboundedReceiver<StreamChunk>, mpsc::UnboundedReceiver<ConnectionStatus>) {
        let (tx, mut rx) = mpsc::unbounded_channel::<ClientMessage>();
        let (chunk_tx, chunk_rx) = mpsc::unbounded_channel::<StreamChunk>();
        let (status_tx, status_rx) = mpsc::unbounded_channel::<ConnectionStatus>();
        let status = Arc::new(Mutex::new(ConnectionStatus::Disconnected));
        
        let client = Self {
            tx,
        };

        tokio::spawn(async move {
            loop {
                let new_status = ConnectionStatus::Connecting;
                *status.lock().await = new_status.clone();
                let _ = status_tx.send(new_status);
                debug!("Attempting to connect to {}", &ws_url);
                
                match connect_async(&ws_url).await {
                    Ok((ws_stream, _)) => {
                        debug!("Connected to backend");
                        let new_status = ConnectionStatus::Connected;
                        *status.lock().await = new_status.clone();
                        let _ = status_tx.send(new_status);
                        
                        let (mut write, mut read) = ws_stream.split();
                        
                        loop {
                            tokio::select! {
                                Some(message) = rx.recv() => {
                                    info!("=== CLIENT SENDING MESSAGE TO SERVER ===");
                                    match &message {
                                        ClientMessage::ChatRequest(req) => {
                                            info!("Message type: ChatRequest");
                                            info!("Number of messages: {}", req.messages.len());
                                        }
                                        ClientMessage::ToolResult { id, content } => {
                                            info!("Message type: ToolResult");
                                            info!("Tool ID: {}", id);
                                            info!("Content length: {} chars", content.len());
                                        }
                                    }

                                    match serde_json::to_string(&message) {
                                        Ok(json) => {
                                            info!("Serialized message ({} bytes)", json.len());
                                            if let Err(e) = write.send(Message::Text(json)).await {
                                                error!("Failed to send message: {}", e);
                                                break;
                                            }
                                            info!("Message sent successfully");
                                        }
                                        Err(e) => {
                                            error!("Failed to serialize request: {}", e);
                                        }
                                    }
                                }
                                Some(msg) = read.next() => {
                                    match msg {
                                        Ok(Message::Text(text)) => {
                                            match serde_json::from_str::<StreamChunk>(&text) {
                                                Ok(chunk) => {
                                                    match &chunk {
                                                        StreamChunk::Text { delta, done } => {
                                                            if *done {
                                                                info!("Received completion chunk from server");
                                                            }
                                                        }
                                                        StreamChunk::ToolCall { id, name, .. } => {
                                                            info!("=== RECEIVED TOOL CALL FROM SERVER ===");
                                                            info!("Tool: {} (ID: {})", name, id);
                                                        }
                                                        StreamChunk::ToolResult { id, .. } => {
                                                            info!("Received tool result from server (ID: {})", id);
                                                        }
                                                    }
                                                    if chunk_tx.send(chunk).is_err() {
                                                        error!("Failed to send chunk to receiver");
                                                        break;
                                                    }
                                                }
                                                Err(e) => {
                                                    error!("Failed to parse chunk: {}", e);
                                                    error!("Raw text was: {}", text);
                                                }
                                            }
                                        }
                                        Ok(Message::Close(_)) => {
                                            warn!("WebSocket connection closed");
                                            break;
                                        }
                                        Err(e) => {
                                            error!("WebSocket error: {}", e);
                                            break;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to connect: {}", e);
                        let new_status = ConnectionStatus::Error(e.to_string());
                        *status.lock().await = new_status.clone();
                        let _ = status_tx.send(new_status);
                    }
                }
                
                let new_status = ConnectionStatus::Disconnected;
                *status.lock().await = new_status.clone();
                let _ = status_tx.send(new_status);
                warn!("Reconnecting in 2 seconds...");
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            }
        });

        (client, chunk_rx, status_rx)
    }

    pub async fn send_message(&self, request: ClientChatRequest) -> Result<()> {
        self.tx.send(ClientMessage::ChatRequest(request))?;
        Ok(())
    }

    pub async fn send_tool_result(&self, id: String, content: String) -> Result<()> {
        self.tx.send(ClientMessage::ToolResult { id, content })?;
        Ok(())
    }
}