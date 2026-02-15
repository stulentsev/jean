# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Jean** is a terminal-based coding agent built in Rust. It uses a client-server architecture: the server wraps the OpenAI API and manages conversation state, while the CLI provides a Ratatui TUI that handles tool execution locally. Communication is over WebSocket with streaming responses and a tool-calling loop.

## Crate Structure

```
jean/                          # Workspace root (Rust 2024 edition)
├── jean-shared/src/lib.rs    # All shared types (ChatMessage, ToolCall, StreamChunk, ClientMessage)
├── jean-server/src/
│   ├── main.rs               # Axum router, WebSocket handler, conversation history management
│   └── llm.rs                # OpenAI client wrapper, tool definitions, streaming
├── jean-cli/src/
│   ├── main.rs               # TUI app (App struct, event loop, tool execution, UI rendering)
│   ├── client.rs             # WebSocket client with auto-reconnection
│   └── conversation_logger.rs # .jsonl conversation logging
└── examples/                  # Ratatui learning examples
```

## Development Commands

```bash
cargo build                     # Build entire workspace
cargo build -p jean-server      # Build specific crate
cargo check                     # Fast compile check
cargo clippy                    # Lint
cargo fmt                       # Format

# Running (requires .env — copy from .env.example)
cargo run -p jean-server        # Terminal 1: backend on 127.0.0.1:3000
cargo run -p jean-cli           # Terminal 2: TUI client

cargo test                      # No tests exist yet
```

## Environment Variables (.env)

```
OPENAI_API_KEY=sk-...           # Required
OPENAI_MODEL=gpt-5-mini         # Model selection
JEAN_WS_HOST=127.0.0.1:3000    # WebSocket host (default: 127.0.0.1:3000)
```

Both `jean-server` and `jean-cli` load `.env` via the `dotenv` crate.

## Architecture

### Communication Flow

1. CLI sends `ClientMessage::ChatRequest` (contains message history) over WebSocket
2. Server appends to per-connection conversation history, calls OpenAI with tool definitions
3. Server streams back `StreamChunk::Text` deltas or `StreamChunk::ToolCall` messages
4. On `ToolCall`: CLI executes the tool locally, sends `ClientMessage::ToolResult` back
5. Server appends tool result to history, calls OpenAI again (loop continues until text response)

### Tool Execution

Tools are defined in `jean-server/src/llm.rs` and executed on the client in `jean-cli/src/main.rs`:

- **`read_file`** — reads file contents via `tokio::fs::read_to_string`
- **`grep`** — regex search across files using the `ignore` crate (respects `.gitignore`), with glob filtering and context lines

The `execute_tool()` function in `jean-cli/src/main.rs` dispatches by tool name. Add new tools there and register their OpenAI function definitions in `llm.rs`.

### Shared Types (`jean-shared`)

- `MessageRole`: System / User / Assistant / Tool (serialized lowercase)
- `ChatMessage`: role + content + optional tool_call_id + optional tool_calls
- `ToolCall`: id + name + arguments (JSON string)
- `ClientMessage`: enum — `ChatRequest` or `ToolResult { id, content }`
- `StreamChunk`: enum — `Text { delta, done }` / `ToolCall { id, name, arguments }` / `ToolResult { id, content }`

### Key Patterns

- **`tokio::select!` event loop** in CLI: multiplexes keyboard input (from OS thread), WebSocket chunks, and connection status
- **Channel-based**: `mpsc::unbounded_channel` connects WebSocket client to TUI; a separate channel carries `ConnectionStatus`
- **Auto-reconnection**: `BackendClient` reconnects with 2-second delay on disconnect
- **Conversation logging**: all messages logged to `conversation_logs/conversation_YYYYMMDD_HHMMSS.jsonl`

## Development Notes

- Client logs to `jean-cli.log` (never use `println!` in CLI code — it corrupts the TUI). Use `tracing` macros.
- Server logs to stdout via `tracing-subscriber`.
- `anyhow::Result` is used throughout for error propagation.
- No tests exist yet in any crate.

## Project Guidelines

- Never mention Claude or Claude Code in git commits or PR descriptions.
