# CLAUDE.md - Project Context

## Project Overview
**Speak Code** is a terminal-based coding assistant similar to Claude Code, built entirely in Rust. It provides an interactive CLI interface for chatting with LLMs to assist with coding tasks.

## Architecture

### Tech Stack
- **Language**: Rust (all components)
- **Backend**: Axum web framework with WebSocket support
- **Frontend**: Ratatui TUI library for terminal interface
- **Communication**: REST API + WebSocket for streaming responses
- **LLM Integration**: Ready for OpenAI/Anthropic APIs via `async-openai`

### Project Structure
```
speak-code/
├── speak-code-server/    # Axum backend server
├── speak-code-cli/       # Ratatui TUI client
├── speak-code-shared/    # Shared types between client/server
└── Cargo.toml           # Workspace configuration
```

### Key Components

**speak-code-shared**
- Defines common types: `ChatMessage`, `ChatRequest`, `ChatResponse`, `StreamChunk`
- Ensures type safety across client/server boundary

**speak-code-server**
- REST endpoint: `POST /chat` for single responses
- WebSocket: `/ws/chat` for streaming responses
- Health check: `GET /health`
- Runs on `http://127.0.0.1:3000`
- TODO: Integrate actual LLM APIs (OpenAI/Anthropic)

**speak-code-cli**
- Terminal UI with message history
- Keyboard controls: Type message, Enter to send, Ctrl-Q to quit
- Arrow keys for scrolling history
- TODO: Connect to backend via WebSocket

## Development Commands

```bash
# Build all components
cargo build

# Run backend server
cargo run -p speak-code-server

# Run CLI (in separate terminal)
cargo run -p speak-code-cli

# Run with release optimizations
cargo run --release -p speak-code-cli
```

## Next Steps / TODOs

1. **LLM Integration**
   - Add API key configuration (.env support)
   - Implement OpenAI/Anthropic client in server
   - Handle streaming responses properly

2. **CLI-Server Connection**
   - Implement WebSocket client in CLI
   - Handle connection errors gracefully
   - Add connection status indicator

3. **Features**
   - Syntax highlighting for code blocks
   - File system access (read/write code files)
   - Multiple conversation threads
   - Configuration file for server URL, model preferences
   - Export conversation history

4. **UI Enhancements**
   - Better text wrapping for long messages
   - Code block rendering with language detection
   - Vim-style keybindings option
   - Split pane view (chat + code editor)

## Design Decisions

- **Full Rust Stack**: Chosen for performance, memory safety, and single-language consistency
- **Ratatui over Bubbletea**: Native Rust ecosystem, great widget library
- **Axum over Rocket**: Better async support, more flexible middleware system
- **WebSocket for Streaming**: Real-time token streaming from LLM responses
- **Shared Types Crate**: Ensures contract between client/server stays in sync

## Repository
- GitHub: `git@github.com:stulentsev/code-speak.git`
- Main branch: `master` 

## Testing Strategy
- Unit tests for shared types and business logic
- Integration tests for API endpoints
- Manual testing for TUI interactions
- Consider property-based testing for message serialization