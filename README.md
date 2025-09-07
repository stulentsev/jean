# Speak Code

A terminal-based coding assistant built with Rust.

## Architecture

- **speak-code-cli**: Ratatui-based TUI client
- **speak-code-server**: Axum backend server with LLM integration  
- **speak-code-shared**: Shared types between client and server

## Quick Start

```bash
# Run the backend
cd speak-code-server
cargo run

# In another terminal, run the CLI
cd speak-code-cli
cargo run
```

## Features

- WebSocket streaming for real-time responses
- Clean terminal UI with message history
- Shared type definitions for type safety
- Ready for OpenAI/Anthropic API integration