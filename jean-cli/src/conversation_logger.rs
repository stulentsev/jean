use anyhow::Result;
use jean_shared::{ChatMessage, StreamChunk};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use chrono::{DateTime, Local};
use tracing::{debug, error};

#[derive(Debug, Serialize, Deserialize)]
pub struct ConversationEntry {
    pub timestamp: DateTime<Local>,
    pub entry_type: EntryType,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EntryType {
    UserMessage {
        content: String,
    },
    AssistantMessage {
        content: String,
    },
    ToolInfo {
        content: String,
    },
    SystemMessage {
        content: String,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    ToolResult {
        id: String,
        content: String,
    },
}

pub struct ConversationLogger {
    _log_dir: PathBuf,
    current_log_file: Option<PathBuf>,
    _session_start: DateTime<Local>,
}

impl ConversationLogger {
    pub fn new() -> Result<Self> {
        let log_dir = PathBuf::from("conversation_logs");
        if !log_dir.exists() {
            fs::create_dir_all(&log_dir)?;
        }

        let session_start = Local::now();
        let filename = format!("conversation_{}.jsonl", session_start.format("%Y%m%d_%H%M%S"));
        let log_file = log_dir.join(filename);

        debug!("Starting conversation logger: {:?}", log_file);

        Ok(Self {
            _log_dir: log_dir,
            current_log_file: Some(log_file),
            _session_start: session_start,
        })
    }

    pub fn log_entry(&self, entry_type: EntryType) -> Result<()> {
        if let Some(ref log_file) = self.current_log_file {
            let entry = ConversationEntry {
                timestamp: Local::now(),
                entry_type,
            };

            let json = serde_json::to_string(&entry)?;

            // Append to file with newline
            use std::io::Write;
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_file)?;

            writeln!(file, "{}", json)?;
            file.flush()?;
        }
        Ok(())
    }

    pub fn log_message(&self, message: &ChatMessage) -> Result<()> {
        let entry_type = match message.role {
            jean_shared::MessageRole::User => EntryType::UserMessage {
                content: message.content.clone(),
            },
            jean_shared::MessageRole::Assistant => EntryType::AssistantMessage {
                content: message.content.clone(),
            },
            jean_shared::MessageRole::System => {
                // Check if this is a ToolInfo message
                if message.content.starts_with("[ToolInfo]") {
                    EntryType::ToolInfo {
                        content: message.content.clone(),
                    }
                } else {
                    EntryType::SystemMessage {
                        content: message.content.clone(),
                    }
                }
            },
            jean_shared::MessageRole::Tool => EntryType::ToolResult {
                id: message.tool_call_id.clone().unwrap_or_default(),
                content: message.content.clone(),
            },
        };
        self.log_entry(entry_type)
    }

    pub fn log_stream_chunk(&self, chunk: &StreamChunk) -> Result<()> {
        // Only log tool calls and tool results from chunks
        // Text streaming chunks are ignored as we'll log the complete message later
        match chunk {
            StreamChunk::Text { .. } => {
                // Don't log streaming text chunks
                Ok(())
            }
            StreamChunk::ToolCall { id, name, arguments } => {
                self.log_entry(EntryType::ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: arguments.clone(),
                })
            }
            StreamChunk::ToolResult { id, content } => {
                self.log_entry(EntryType::ToolResult {
                    id: id.clone(),
                    content: content.clone(),
                })
            }
        }
    }

    pub fn log_connection_status(&self, _status: &str) -> Result<()> {
        // Don't log connection status changes
        Ok(())
    }

    pub fn log_tool_execution(&self, tool_id: &str, tool_name: &str, result: &str) -> Result<()> {
        self.log_entry(EntryType::ToolResult {
            id: tool_id.to_string(),
            content: format!("[{}] {}", tool_name, result),
        })
    }

    pub fn get_current_log_path(&self) -> Option<&Path> {
        self.current_log_file.as_deref()
    }
}

impl Default for ConversationLogger {
    fn default() -> Self {
        Self::new().unwrap_or_else(|e| {
            error!("Failed to create conversation logger: {}", e);
            Self {
                _log_dir: PathBuf::from("conversation_logs"),
                current_log_file: None,
                _session_start: Local::now(),
            }
        })
    }
}