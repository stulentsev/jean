# Jean

A terminal-based coding assistant built with Rust.

## Architecture

- **jean-cli**: Ratatui-based TUI client
- **jean-server**: Axum backend server with LLM integration  
- **jean-shared**: Shared types between client and server

## Quick Start

Prepare the .env file (don't forget to put your real api key in)

```bash
cp .env.example .env
```

Then

```bash
# Run the backend
cd jean-server
cargo run

# In another terminal, run the CLI
cd jean-cli
cargo run
```

## Features

- WebSocket streaming for real-time responses
- Clean terminal UI with message history
- Shared type definitions for type safety
- Ready for OpenAI/Anthropic API integration