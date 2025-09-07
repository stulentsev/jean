# Simple Chat Feature Implementation Plan

## Overview
Implement end-to-end chat functionality with streaming responses from OpenAI API, enabling real-time conversation between the CLI client and LLM backend.

## Architecture Decision
**LLM Provider**: OpenAI (GPT-4)
- Rationale: Excellent streaming support, well-documented API, robust Rust client library (`async-openai`)

## Implementation Steps

### Phase 1: Backend LLM Integration

#### 1.1 Add Dependencies
**File**: `speak-code-server/Cargo.toml`
- Add `async-openai = "0.24"`
- Add `dotenv = "0.15"` for environment configuration
- Add `futures-util = "0.3"` for stream handling

#### 1.2 Environment Configuration
**File**: `.env` (gitignored)
```
OPENAI_API_KEY=sk-...
OPENAI_MODEL=gpt-4-turbo-preview
```

#### 1.3 LLM Service Module
**File**: `speak-code-server/src/llm.rs`
- Create OpenAI client wrapper
- Implement streaming response handler
- Convert OpenAI stream chunks to our `StreamChunk` format
- Handle errors gracefully (API limits, network issues)

#### 1.4 Update WebSocket Handler
**File**: `speak-code-server/src/main.rs`
- Integrate LLM service into `handle_socket`
- Stream OpenAI responses chunk by chunk
- Send each chunk as WebSocket message
- Send final chunk with `done: true`

### Phase 2: CLI WebSocket Client

#### 2.1 Add Dependencies
**File**: `speak-code-cli/Cargo.toml`
- Add `tokio-tungstenite = "0.24"` for WebSocket client
- Add `futures-util = "0.3"` for stream handling
- Add `reqwest = { version = "0.12", features = ["json"] }` for HTTP client

#### 2.2 Backend Client Module
**File**: `speak-code-cli/src/client.rs`
- WebSocket connection management
- Reconnection logic with exponential backoff
- Message serialization/deserialization
- Stream chunk accumulation

#### 2.3 Update App State
**File**: `speak-code-cli/src/main.rs`
- Add connection status field
- Add pending message buffer
- Add streaming message accumulator
- Track message state (sending/streaming/complete)

#### 2.4 Async Event Loop
**File**: `speak-code-cli/src/main.rs`
- Convert main loop to use tokio channels
- Separate UI events from network events
- Handle concurrent keyboard input and WebSocket messages
- Update UI on each stream chunk arrival

### Phase 3: UI Enhancements

#### 3.1 Streaming Display
- Show partial messages as they arrive
- Add typing indicator ("●●●") during streaming
- Smooth text append without flickering
- Auto-scroll to latest message

#### 3.2 Connection Status
- Display connection state in UI header
- Show "Connecting...", "Connected", "Disconnected"
- Visual indicator (color-coded)

#### 3.3 Error Handling
- Display network errors gracefully
- Show retry attempts
- Allow manual reconnection (Ctrl-R)

## Technical Details

### Message Flow
1. User types message and hits Enter
2. CLI sends `ChatRequest` via WebSocket
3. Server receives request, calls OpenAI streaming API
4. Server forwards each chunk as `StreamChunk` via WebSocket
5. CLI accumulates chunks, updates display in real-time
6. Final chunk with `done: true` completes the message

### Data Structures

#### Enhanced StreamChunk
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    pub delta: String,
    pub done: bool,
    pub error: Option<String>,
}
```

#### Connection State
```rust
enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}
```

### Error Scenarios
1. **Network Failure**: Auto-reconnect with backoff
2. **API Rate Limit**: Queue messages, retry with delay
3. **Invalid API Key**: Display error, prompt for configuration
4. **WebSocket Timeout**: Reconnect automatically
5. **Malformed Response**: Log error, skip chunk

## Testing Strategy

### Unit Tests
- LLM service mock responses
- WebSocket message serialization
- Stream chunk accumulation logic

### Integration Tests
- End-to-end message flow (with mock OpenAI)
- Reconnection scenarios
- Error handling paths

### Manual Testing
- Various message lengths
- Rapid message sending
- Network interruption simulation
- Long streaming responses

## Configuration

### Server Configuration
```toml
# speak-code-server/config.toml
[openai]
api_key_env = "OPENAI_API_KEY"
model = "gpt-4-turbo-preview"
max_tokens = 2000
temperature = 0.7
stream_timeout_secs = 30
```

### Client Configuration
```toml
# speak-code-cli/config.toml
[server]
url = "ws://127.0.0.1:3000/ws/chat"
reconnect_interval_ms = 1000
max_reconnect_attempts = 5
```

## Deliverables

1. **Working streaming chat**: Messages flow from CLI → Server → OpenAI → Server → CLI
2. **Robust error handling**: Graceful degradation on failures
3. **Smooth UX**: Real-time streaming display without flickering
4. **Configuration**: Environment-based setup for API keys
5. **Documentation**: Updated README with setup instructions

## Implementation Order

1. **Backend first**: Get OpenAI integration working with curl/Postman testing
2. **CLI WebSocket**: Establish basic connection and message exchange
3. **Streaming flow**: Implement chunk-by-chunk updates
4. **Polish**: Error handling, reconnection, UI improvements
5. **Testing**: Comprehensive test coverage

## Time Estimate
- Phase 1 (Backend): 2-3 hours
- Phase 2 (CLI Client): 3-4 hours  
- Phase 3 (UI): 2-3 hours
- Testing & Polish: 2 hours
- **Total**: 9-12 hours

## Success Criteria
- [ ] Can send message from CLI and receive streaming response
- [ ] Response appears character by character (or word by word)
- [ ] Connection automatically recovers from network issues
- [ ] Clear error messages for configuration problems
- [ ] No UI flickering during streaming
- [ ] Messages persist in chat history
- [ ] Can have multi-turn conversations