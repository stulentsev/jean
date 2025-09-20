use async_openai::{
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs,
        ChatCompletionRequestUserMessageArgs, ChatCompletionRequestAssistantMessageArgs,
        ChatCompletionRequestToolMessageArgs,
        ChatCompletionMessageToolCall,
        FunctionCall,
        CreateChatCompletionRequestArgs, ChatCompletionTool, FunctionObject, ChatCompletionToolType,
    },
    Client,
};
use futures_util::StreamExt;
use jean_shared::{ChatMessage, MessageRole, StreamChunk};
use std::error::Error;
use tokio::sync::mpsc;
use tracing::{info, error};

pub struct LlmService {
    client: Client<OpenAIConfig>,
    model: String,
}

impl LlmService {
    pub fn new(api_key: String, model: String) -> Self {
        info!("Initializing LLM service with model: {}", model);
        let config = OpenAIConfig::new().with_api_key(api_key);
        let client = Client::with_config(config);
        Self { client, model }
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub async fn stream_chat(
        &self,
        messages: Vec<ChatMessage>,
    ) -> Result<mpsc::UnboundedReceiver<StreamChunk>, Box<dyn Error + Send + Sync>> {
        let user_messages = messages
            .into_iter()
            .map(|msg| self.convert_to_openai_message(msg))
            .collect::<Result<Vec<_>, _>>()?;

        let system_message = ChatCompletionRequestMessage::System(
            ChatCompletionRequestSystemMessageArgs::default()
                .content(self.system_prompt())
                .build()?
        );

        let mut messages = Vec::with_capacity(1 + user_messages.len());
        messages.push(system_message);
        messages.extend(user_messages);

        
        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .messages(messages)
            .tools(self.tool_definitions())
            .stream(true)
            .build()?;

        // Dump the actual JSON that will be sent
        let request_json = serde_json::to_string_pretty(&request)?;
        info!("=== JSON PAYLOAD TO OPENAI ===");
        info!("{}", request_json);
        info!("=== END JSON PAYLOAD ===");

        let stream_result = self.client.chat().create_stream(request).await;
        
        let mut stream = match stream_result {
            Ok(s) => {
                s
            },
            Err(e) => {
                error!("Failed to create OpenAI stream: {:?}", e);
                // Try to extract more details about the error
                let error_details = format!("{:?}", e);
                error!("Full error details: {}", error_details);
                return Err(Box::new(e));
            }
        };
        
        let (tx, rx) = mpsc::unbounded_channel();

        tokio::spawn(async move {
            let mut tool_calls: Vec<ChatCompletionMessageToolCall> = Vec::new();
            let mut sent_tool_calls = false;

            while let Some(result) = stream.next().await {
                match result {
                    Ok(response) => {
                        if let Some(choice) = response.choices.first() {
                            // Handle text content
                            if let Some(delta) = &choice.delta.content {
                                let chunk = StreamChunk::Text {
                                    delta: delta.clone(),
                                    done: false,
                                };
                                if tx.send(chunk).is_err() {
                                    error!("Failed to send chunk to channel");
                                    break;
                                }
                            }

                            // Handle tool calls
                            if let Some(delta_tool_calls) = &choice.delta.tool_calls {
                                for delta_tool in delta_tool_calls {
                                    // Find or create tool call entry
                                    let index = delta_tool.index as usize;
                                    while tool_calls.len() <= index {
                                        tool_calls.push(ChatCompletionMessageToolCall {
                                            id: String::new(),
                                            r#type: ChatCompletionToolType::Function,
                                            function: FunctionCall {
                                                name: String::new(),
                                                arguments: String::new(),
                                            },
                                        });
                                    }

                                    // Update tool call with delta
                                    if let Some(id) = &delta_tool.id {
                                        tool_calls[index].id = id.clone();
                                    }
                                    if let Some(function) = &delta_tool.function {
                                        if let Some(name) = &function.name {
                                            tool_calls[index].function.name = name.clone();
                                        }
                                        if let Some(args) = &function.arguments {
                                            tool_calls[index].function.arguments.push_str(args);
                                        }
                                    }
                                }
                            }

                            // Check if we have complete tool calls (finish_reason is "tool_calls")
                            if let Some(finish_reason) = &choice.finish_reason {
                                if finish_reason == &async_openai::types::FinishReason::ToolCalls {
                                    info!("=== TOOL CALLS DETECTED ===");
                                    info!("Number of tool calls: {}", tool_calls.len());

                                    // Send tool calls to client for execution
                                    for tool_call in &tool_calls {
                                        info!("Sending tool call to client:");
                                        info!("  Tool ID: {}", tool_call.id);
                                        info!("  Tool Name: {}", tool_call.function.name);
                                        info!("  Arguments: {}", tool_call.function.arguments);

                                        let chunk = StreamChunk::ToolCall {
                                            id: tool_call.id.clone(),
                                            name: tool_call.function.name.clone(),
                                            arguments: tool_call.function.arguments.clone(),
                                        };

                                        let chunk_json = serde_json::to_string_pretty(&chunk).unwrap_or_else(|_| "Failed to serialize".to_string());
                                        info!("Tool call chunk JSON:\n{}", chunk_json);

                                        if tx.send(chunk).is_err() {
                                            error!("Failed to send tool call chunk");
                                            break;
                                        }
                                        sent_tool_calls = true;
                                    }

                                    // Clear tool calls for next iteration
                                    // Note: The client will execute the tools and send results back
                                    tool_calls.clear();
                                }
                            }
                        }
                    }
                    
                    Err(e) => {
                        error!("OpenAI stream error: {:?}", e);
                        let error_msg = match &e {
                            async_openai::error::OpenAIError::ApiError(api_err) => {
                                format!("OpenAI API Error: {} (Code: {:?}, Type: {:?})", 
                                    api_err.message, api_err.code, api_err.r#type)
                            },
                            _ => format!("OpenAI Error: {:?}", e)
                        };
                        error!("Detailed error: {}", error_msg);
                        let error_chunk = StreamChunk::Text {
                            delta: error_msg,
                            done: true,
                        };
                        let _ = tx.send(error_chunk);
                        break;
                    }
                }
            }

            let done_chunk = StreamChunk::Text {
                delta: String::new(),
                done: true,
            };
            if tx.send(done_chunk).is_err() {
                error!("Failed to send done chunk");
            }
        });

        Ok(rx)
    }

    fn system_prompt(&self) -> String {
        format!(
            "You are a coding assistant. Your goal is to complete the coding task given to you by USER.\n\
            You can and should use provided tools to complete the task."
        )
    }

    fn tool_definitions(&self) -> Vec<ChatCompletionTool> {
        let read_file = ChatCompletionTool {
            r#type: ChatCompletionToolType::Function,
            function: FunctionObject {
                name: "read_file".to_string(),
                description: Some("Read a file and return the contents".to_string()),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "filename": {
                            "type": "string",
                            "description": "Absolute or workspace-relative path of the file to read"
                        }
                    },
                    "required": ["filename"],
                    "additionalProperties": false
                }).into(),
                strict: None
            },
        };

        let grep = ChatCompletionTool {
            r#type: ChatCompletionToolType::Function,
            function: FunctionObject {
                name: "grep".to_string(),
                description: Some("Search for content in files using regex patterns".to_string()),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "search_term": {
                            "type": "string",
                            "description": "Search term (can be a regex pattern)"
                        },
                        "filter": {
                            "type": "string",
                            "description": "File filter pattern (e.g., 'src/**/*.rs', '*.txt')"
                        },
                        "context_lines": {
                            "type": "integer",
                            "description": "Number of lines to show before and after each match",
                            "default": 2
                        }
                    },
                    "required": ["search_term", "filter"],
                    "additionalProperties": false
                }).into(),
                strict: None
            },
        };

        vec![read_file, grep]
    }

    fn convert_to_openai_message(
        &self,
        msg: ChatMessage,
    ) -> Result<ChatCompletionRequestMessage, Box<dyn Error + Send + Sync>> {
        let message = match msg.role {
            MessageRole::System => {
                ChatCompletionRequestMessage::System(
                    ChatCompletionRequestSystemMessageArgs::default()
                        .content(msg.content)
                        .build()?
                )
            }
            MessageRole::User => {
                ChatCompletionRequestMessage::User(
                    ChatCompletionRequestUserMessageArgs::default()
                        .content(msg.content)
                        .build()?
                )
            }
            MessageRole::Assistant => {
                let mut builder = ChatCompletionRequestAssistantMessageArgs::default();

                // Only set content if it's not empty
                if !msg.content.is_empty() {
                    builder.content(msg.content);
                }

                // If there are tool calls, add them
                if let Some(tool_calls) = msg.tool_calls {
                    let calls: Vec<ChatCompletionMessageToolCall> = tool_calls
                        .into_iter()
                        .map(|tc| ChatCompletionMessageToolCall {
                            id: tc.id,
                            r#type: ChatCompletionToolType::Function,
                            function: FunctionCall {
                                name: tc.name,
                                arguments: tc.arguments,
                            },
                        })
                        .collect();
                    builder.tool_calls(calls);
                }

                ChatCompletionRequestMessage::Assistant(builder.build()?)
            }
            MessageRole::Tool => {
                ChatCompletionRequestMessage::Tool(
                    ChatCompletionRequestToolMessageArgs::default()
                        .content(msg.content)
                        .tool_call_id(msg.tool_call_id.unwrap_or_default())
                        .build()?
                )
            }
        };
        Ok(message)
    }
}