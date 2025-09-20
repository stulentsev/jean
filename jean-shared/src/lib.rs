use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
}

/// Message from client to server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    #[serde(rename = "chat_request")]
    ChatRequest(ClientChatRequest),
    #[serde(rename = "tool_result")]
    ToolResult {
        id: String,
        content: String,
    },
}

/// Request from client to server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientChatRequest {
    pub messages: Vec<ChatMessage>,
    // Future: tool_ids, context_window, etc.
}

/// Internal request from server to LLM - kept for future use when
/// server needs to make direct LLM calls with model selection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    pub model: String,
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub content: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StreamChunk {
    #[serde(rename = "text")]
    Text {
        delta: String,
        done: bool,
    },
    #[serde(rename = "tool_call")]
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        id: String,
        content: String,
    },
}