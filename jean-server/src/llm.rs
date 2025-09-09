use async_openai::{
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs,
        ChatCompletionRequestUserMessageArgs, ChatCompletionRequestAssistantMessageArgs,
        CreateChatCompletionRequestArgs,
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

    pub async fn stream_chat(
        &self,
        messages: Vec<ChatMessage>,
    ) -> Result<mpsc::UnboundedReceiver<StreamChunk>, Box<dyn Error + Send + Sync>> {
        let openai_messages = messages
            .into_iter()
            .map(|msg| self.convert_to_openai_message(msg))
            .collect::<Result<Vec<_>, _>>()?;
        
        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .messages(openai_messages)
            .stream(true)
            .build()?;

        // Dump the actual JSON that will be sent
        let request_json = serde_json::to_string_pretty(&request)?;
        info!("JSON payload to OpenAI:\n{}", request_json);

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
            while let Some(result) = stream.next().await {
                match result {
                    Ok(response) => {
                        if let Some(choice) = response.choices.first() {
                            if let Some(delta) = &choice.delta.content {
                                let chunk = StreamChunk {
                                    delta: delta.clone(),
                                    done: false,
                                };
                                if tx.send(chunk).is_err() {
                                    error!("Failed to send chunk to channel");
                                    break;
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
                        let error_chunk = StreamChunk {
                            delta: error_msg,
                            done: true,
                        };
                        let _ = tx.send(error_chunk);
                        break;
                    }
                }
            }
            
            let done_chunk = StreamChunk {
                delta: String::new(),
                done: true,
            };
            let _ = tx.send(done_chunk);
        });

        Ok(rx)
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
                ChatCompletionRequestMessage::Assistant(
                    ChatCompletionRequestAssistantMessageArgs::default()
                        .content(msg.content)
                        .build()?
                )
            }
        };
        Ok(message)
    }
}