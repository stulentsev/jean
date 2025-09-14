# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview
**Jean** is a terminal-based coding agent built entirely in Rust. It is designed to autonomously complete coding tasks using available tools, pausing only when user input is required. The system provides rich tool-calling capabilities through both client-side (MCP servers) and server-side (codebase analysis, file operations) integrations. The project uses a client-server architecture with WebSocket streaming for real-time responses and tool execution feedback.

## Architecture

### Tech Stack
- **Language**: Rust 2024 edition (workspace with shared dependencies)
- **Backend**: Axum web framework with WebSocket support
- **Frontend**: Ratatui TUI library with crossterm for terminal control
- **Communication**: WebSocket streaming (`ws://127.0.0.1:3000/ws/chat`)
- **LLM Integration**: OpenAI API via `async-openai` crate

### Crate Structure
```
jean/                    # Workspace root
├── jean-shared/        # Shared types (ChatMessage, ChatRequest, StreamChunk)
├── jean-server/        # Axum backend with OpenAI integration
├── jean-cli/           # Ratatui TUI client (binary name: "jean")
└── examples/           # Ratatui example code
```

### Key Architectural Patterns
- **Shared Types Contract**: All communication types defined in `jean-shared` crate
- **Async Everything**: Full tokio async runtime with `tokio::select!` for concurrent events
- **Channel-based Communication**: Async channels between UI thread and WebSocket client
- **Auto-reconnection**: WebSocket client automatically reconnects on failure
- **Streaming Responses**: Real-time character-by-character display from OpenAI

## Development Commands

```bash
# Build & Check
cargo build                     # Build entire workspace
cargo build -p jean-server      # Build specific crate
cargo check                     # Fast compile check
cargo clippy                    # Lint warnings
cargo fmt                       # Format code

# Run Application
cargo run -p jean-server        # Terminal 1: Start backend (requires .env)
cargo run -p jean-cli           # Terminal 2: Start TUI client

# Testing & Debugging
./test_model.sh                 # Test OpenAI API directly
tail -f jean-cli.log           # Monitor client logs (TUI logs to file)
cargo test                      # Run tests (none exist yet)
```

## Configuration

### Environment Variables (`.env` file required for server)
```
OPENAI_API_KEY=sk-...          # OpenAI API key
OPENAI_MODEL=gpt-5-mini        # Model to use (gpt-5-mini, gpt-5, gpt-5-nano)
```

### Important Files
- `jean-cli.log`: Client debug logs (UI logs to file to avoid terminal corruption)
- `test_model.sh`: Bash script for testing OpenAI API directly

## Core Components

### jean-shared Types
- `MessageRole`: System/User/Assistant (serialized as lowercase)
- `ChatMessage`: Role + content
- `ChatRequest`: Messages array + model + stream flag
- `StreamChunk`: Delta string + done flag for streaming

### jean-server Implementation
- **Entry**: `main.rs` sets up Axum router with CORS
- **Handlers**: `handlers.rs` implements WebSocket chat endpoint
- **LLM Service**: `llm_service.rs` wraps OpenAI client with streaming
- **WebSocket**: Converts OpenAI stream to WebSocket messages

### jean-cli Implementation
- **App State**: `app.rs` manages messages, input, scroll, connection status
- **Backend Client**: `backend.rs` handles WebSocket with auto-reconnect
- **Event Loop**: `main.rs` uses `tokio::select!` for UI + WebSocket events
- **UI Rendering**: `ui.rs` builds Ratatui widgets

## Current Implementation Status

### ✅ Working Features
- Full WebSocket streaming from OpenAI to terminal
- Auto-reconnection on network failures
- Scrollable message history with arrow keys
- Connection status indicator
- Real-time streaming with typing indicator

### ⚠️ Known Issues
- No tests exist in any crate
- WebSocket URL hardcoded to `ws://127.0.0.1:3000/ws/chat`
- No configuration file support (only environment variables)
- No tool-calling implementation yet

## Important Development Notes

### Error Handling
- Uses `anyhow::Result` throughout for error propagation
- Errors become system messages in chat UI
- Comprehensive tracing with `tracing` crate

### Logging Strategy
- Server logs to stdout (use `tracing-subscriber`)
- Client logs to file (`jean-cli.log`) to avoid TUI corruption
- Never use `println!` in CLI code - use `tracing` macros

### WebSocket Protocol
- Client sends `ChatRequest` JSON
- Server streams back `StreamChunk` messages
- `done: true` in chunk signals completion
- Connection status tracked and displayed in UI

### Testing Approach
When adding tests:
- Unit test shared types serialization
- Integration test WebSocket endpoints
- Mock OpenAI responses for server tests
- TUI testing requires special terminal handling
- Tool execution tests with mocked file system
- MCP server integration tests

## Tool-Calling Architecture (To Be Implemented)

### Overview
Jean is designed as an autonomous coding agent that uses tools to complete tasks. The architecture supports two types of tool integrations:
- **Client-Side Tools**: MCP (Model Context Protocol) servers for extensible capabilities
- **Server-Side Tools**: Built-in tools for codebase analysis and file operations

### Client-Side Tools (MCP Integration)
- **MCP Server Support**: Connect to Model Context Protocol servers for extended capabilities
- **Tool Discovery**: Dynamic discovery of available tools from MCP servers
- **Tool Execution**: Client-side execution with result streaming back to server
- **Security**: Sandboxed execution with user confirmation for sensitive operations
- **Protocol**: JSON-RPC 2.0 over stdio/WebSocket for MCP communication

### Server-Side Tools
#### Codebase Analysis
- `grep` - Search for patterns across files using ripgrep
- `find` - Locate files by name, type, or attributes
- `ast_search` - Search by AST patterns (language-aware)
- `dependency_graph` - Analyze project dependencies
- `symbol_lookup` - Find function/class definitions

#### File Operations
- `read_file` - Read file contents with line numbers
- `write_file` - Create or overwrite files
- `edit_file` - Make targeted edits with diff preview
- `patch_file` - Apply unified diff patches
- `create_directory` - Create project structure

#### Development Tools
- `run_command` - Execute shell commands with timeout and streaming output
- `test_runner` - Run tests and parse structured results
- `linter` - Run linting tools and format output
- `build` - Compile project and capture errors
- `debugger` - Set breakpoints and inspect state

#### Version Control
- `git_status` - Check repository state
- `git_diff` - Show staged and unstaged changes
- `git_commit` - Create commits with generated messages
- `git_branch` - Manage branches
- `git_log` - View commit history

### Tool-Calling Protocol
```rust
// Tool request from LLM
struct ToolCall {
    id: String,
    tool: String,
    parameters: serde_json::Value,
    requires_confirmation: bool,
}

// Tool result streamed back
struct ToolResult {
    id: String,
    status: ToolStatus,
    output: Option<String>,
    error: Option<String>,
}
```

### Implementation Phases
1. **Phase 1**: Basic file operations (read, write, edit) with WebSocket protocol
2. **Phase 2**: Codebase search and analysis tools
3. **Phase 3**: Development tools (test, build, lint) with streaming output
4. **Phase 4**: MCP server integration for extensibility
5. **Phase 5**: Advanced agent planning and multi-step task execution

### Agent Capabilities (Future)
- **Task Planning**: Decompose complex tasks into executable steps
- **Context Management**: Intelligently gather and manage relevant context
- **Error Recovery**: Retry failed operations with alternative approaches
- **Learning**: Remember project-specific patterns and user preferences
- **Collaboration**: Work alongside user with minimal interruption
- **Tool Chaining**: Execute tools in sequence or parallel based on dependencies

## Project Guidelines

- Never mention Claude or Claude Code in git commits or PR descriptions