mod client;
mod conversation_logger;

use anyhow::Result;
use conversation_logger::ConversationLogger;
use client::{BackendClient, ConnectionStatus};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};
use jean_shared::{ChatMessage, ClientChatRequest, MessageRole, StreamChunk};
use serde::{Deserialize, Serialize};
use std::io;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

// Tool argument structs
#[derive(Debug, Deserialize, Serialize)]
struct ReadFileArgs {
    filename: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct GrepArgs {
    search_term: String,
    filter: String,
    #[serde(default = "default_context_lines")]
    context_lines: usize,
}

fn default_context_lines() -> usize {
    2
}

struct App {
    messages: Vec<ChatMessage>,
    input: String,
    scroll_offset: usize,
    connection_status: ConnectionStatus,
    streaming_message: Option<String>,
    cursor_position: usize,
    expecting_tool_response: bool,  // Track if we're waiting for response after tool execution
    logger: ConversationLogger,
}

impl App {
    fn new() -> Self {
        let logger = ConversationLogger::new().unwrap_or_else(|e| {
            error!("Failed to create conversation logger: {}", e);
            ConversationLogger::default()
        });

        if let Some(path) = logger.get_current_log_path() {
            info!("Logging conversation to: {:?}", path);
        }

        Self {
            messages: vec![],
            input: String::new(),
            scroll_offset: 0,
            connection_status: ConnectionStatus::Disconnected,
            streaming_message: None,
            cursor_position: 0,
            expecting_tool_response: false,
            logger,
        }
    }

    fn add_user_message(&mut self, content: String) {
        let message = ChatMessage {
            role: MessageRole::User,
            content,
            tool_call_id: None,
            tool_calls: None,
        };

        // Log the message
        if let Err(e) = self.logger.log_message(&message) {
            error!("Failed to log user message: {}", e);
        }

        self.messages.push(message);
    }

    fn start_streaming(&mut self) {
        self.streaming_message = Some(String::new());
    }

    fn append_stream_chunk(&mut self, chunk: &str) {
        if let Some(ref mut msg) = self.streaming_message {
            msg.push_str(chunk);
        }
    }

    fn finish_streaming(&mut self) {
        if let Some(content) = self.streaming_message.take() {
            if !content.is_empty() {
                let message = ChatMessage {
                    role: MessageRole::Assistant,
                    content,
                    tool_call_id: None,
                    tool_calls: None,
                };

                // Log the complete assistant message
                if let Err(e) = self.logger.log_message(&message) {
                    error!("Failed to log assistant message: {}", e);
                }

                self.messages.push(message);
            }
        }
    }

    fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
        }
    }

    fn move_cursor_right(&mut self) {
        if self.cursor_position < self.input.len() {
            self.cursor_position += 1;
        }
    }

    fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor_position, c);
        self.cursor_position += 1;
    }

    fn delete_char(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
            self.input.remove(self.cursor_position);
        }
    }

    fn scroll_up(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(amount);
    }

    fn scroll_down(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging to file instead of terminal to avoid corrupting TUI
    let log_file = std::fs::File::create("jean-cli.log").ok();
    if let Some(file) = log_file {
        tracing_subscriber::fmt()
            .with_writer(file)
            .with_ansi(false)
            .init();
    }
    
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    
    let ws_url = "ws://127.0.0.1:3000/ws/chat".to_string();
    let (client, mut chunk_rx, mut status_rx) = BackendClient::new(ws_url);
    
    let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
    
    std::thread::spawn(move || {
        while let Ok(event) = event::read() {
            if ui_tx.send(event).is_err() {
                break;
            }
        }
    });
    
    let res = run_app(&mut terminal, &mut app, client, &mut chunk_rx, &mut status_rx, &mut ui_rx).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{err:?}");
    }

    Ok(())
}

async fn execute_tool(name: &str, arguments: &str) -> String {
    match name {
        "read_file" => {
            // Parse arguments with typed struct
            let args: ReadFileArgs = match serde_json::from_str(arguments) {
                Ok(args) => args,
                Err(e) => {
                    return format!("Error parsing read_file arguments: {}", e);
                }
            };

            // Read the file
            match tokio::fs::read_to_string(&args.filename).await {
                Ok(content) => content,
                Err(e) => format!("Error reading file '{}': {}", args.filename, e),
            }
        }
        "grep" => {
            // Parse arguments with typed struct
            let args: GrepArgs = match serde_json::from_str(arguments) {
                Ok(args) => args,
                Err(e) => {
                    return format!("Error parsing grep arguments: {}", e);
                }
            };

            execute_grep(args).await
        }
        _ => format!("Unknown tool: {}", name),
    }
}

async fn execute_grep(args: GrepArgs) -> String {
    use ignore::WalkBuilder;
    use regex::Regex;
    use tokio::io::{AsyncBufReadExt, BufReader};
    use glob::Pattern;

    // Compile regex
    let regex = match Regex::new(&args.search_term) {
        Ok(r) => r,
        Err(e) => {
            return format!("Invalid regex pattern '{}': {}", args.search_term, e);
        }
    };

    // Compile glob pattern for filtering
    let glob_pattern = match Pattern::new(&args.filter) {
        Ok(p) => p,
        Err(e) => {
            return format!("Invalid filter pattern '{}': {}", args.filter, e);
        }
    };

    let mut results = Vec::new();

    // Build a walker that respects .gitignore
    let mut builder = WalkBuilder::new(".");
    builder
        .standard_filters(true) // Respects .gitignore, .ignore, etc.
        .hidden(false) // Don't skip hidden files by default (let gitignore handle it)
        .git_ignore(true) // Explicitly enable gitignore support
        .git_global(true) // Also respect global gitignore
        .git_exclude(true); // Also respect .git/info/exclude

    // Walk through files
    for entry in builder.build() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        // Check if the file matches the filter pattern
        if !glob_pattern.matches_path(path) {
            continue;
        }

        // Read file and search for matches
        let file = match tokio::fs::File::open(path).await {
            Ok(f) => f,
            Err(_) => continue,
        };

        let reader = BufReader::new(file);
        let mut lines_reader = reader.lines();
        let mut lines_buffer: Vec<String> = Vec::new();
        let mut line_num: usize = 0;

        while let Ok(Some(line)) = lines_reader.next_line().await {
            line_num += 1;
            lines_buffer.push(line.clone());

            // Keep only necessary context lines in buffer
            if lines_buffer.len() > args.context_lines + 1 {
                lines_buffer.remove(0);
            }

            // Check if current line matches
            if regex.is_match(&line) {
                let mut match_context = Vec::new();

                // Add file path
                match_context.push(format!("=== {} ===", path.display()));

                // Calculate line numbers for context
                let start_offset = lines_buffer.len().saturating_sub(1);
                let start_line = line_num.saturating_sub(start_offset);

                // Add lines with line numbers
                for (i, context_line) in lines_buffer.iter().enumerate() {
                    let current_line_num = start_line + i;
                    if current_line_num == line_num {
                        // Highlight the matching line
                        match_context.push(format!("{}:> {}", current_line_num, context_line));
                    } else {
                        match_context.push(format!("{}:  {}", current_line_num, context_line));
                    }
                }

                // Read ahead for context lines after match
                let mut after_context = Vec::new();
                for _ in 0..args.context_lines {
                    if let Ok(Some(next_line)) = lines_reader.next_line().await {
                        line_num += 1;
                        after_context.push(format!("{}:  {}", line_num, next_line));
                        lines_buffer.push(next_line);
                        if lines_buffer.len() > args.context_lines + 1 {
                            lines_buffer.remove(0);
                        }
                    }
                }

                match_context.extend(after_context);
                results.push(match_context.join("\n"));
            }
        }
    }

    if results.is_empty() {
        format!("No matches found for '{}' in files matching '{}'", args.search_term, args.filter)
    } else {
        format!("Found {} matches:\n\n{}", results.len(), results.join("\n\n"))
    }
}

async fn run_app<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    client: BackendClient,
    chunk_rx: &mut mpsc::UnboundedReceiver<StreamChunk>,
    status_rx: &mut mpsc::UnboundedReceiver<ConnectionStatus>,
    ui_rx: &mut mpsc::UnboundedReceiver<Event>,
) -> Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        tokio::select! {
            Some(new_status) = status_rx.recv() => {
                // Log connection status change
                let status_str = match &new_status {
                    ConnectionStatus::Connected => "Connected",
                    ConnectionStatus::Connecting => "Connecting",
                    ConnectionStatus::Disconnected => "Disconnected",
                    ConnectionStatus::Error(e) => &format!("Error: {}", e),
                };
                if let Err(e) = app.logger.log_connection_status(status_str) {
                    error!("Failed to log connection status: {}", e);
                }
                app.connection_status = new_status;
            }
            Some(event) = ui_rx.recv() => {
                match event {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        match key.code {
                            KeyCode::Char('q') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                                return Ok(())
                            }
                            KeyCode::Char('c') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                                return Ok(())
                            }
                            KeyCode::Char(c) => {
                                app.insert_char(c);
                            }
                            KeyCode::Backspace => {
                                app.delete_char();
                            }
                            KeyCode::Left => {
                                app.move_cursor_left();
                            }
                            KeyCode::Right => {
                                app.move_cursor_right();
                            }
                            KeyCode::Enter => {
                                if !app.input.is_empty() {
                                    let content = app.input.clone();
                                    app.add_user_message(content.clone());
                                    app.input.clear();
                                    app.cursor_position = 0;
                                    app.scroll_to_bottom();
                                    
                                    // Filter out UI-only system messages (ToolInfo)
                                    let messages_to_send: Vec<ChatMessage> = app.messages
                                        .iter()
                                        .filter(|msg| {
                                            // Exclude system messages that are ToolInfo (UI-only)
                                            !(msg.role == MessageRole::System && msg.content.starts_with("[ToolInfo]"))
                                        })
                                        .cloned()
                                        .collect();
                                    
                                    let request = ClientChatRequest {
                                        messages: messages_to_send,
                                    };
                                    
                                    if let Err(e) = client.send_message(request).await {
                                        let error_msg = ChatMessage {
                                            role: MessageRole::System,
                                            content: format!("Failed to send message: {}", e),
                                            tool_call_id: None,
                                            tool_calls: None,
                                        };
                                        if let Err(log_err) = app.logger.log_message(&error_msg) {
                                            error!("Failed to log error message: {}", log_err);
                                        }
                                        app.messages.push(error_msg);
                                    } else {
                                        app.start_streaming();
                                    }
                                }
                            }
                            KeyCode::Up => {
                                app.scroll_up(1);
                            }
                            KeyCode::Down => {
                                app.scroll_down(1);
                            }
                            KeyCode::PageUp => {
                                app.scroll_up(10);
                            }
                            KeyCode::PageDown => {
                                app.scroll_down(10);
                            }
                            KeyCode::Home => {
                                app.cursor_position = 0;
                            }
                            KeyCode::End => {
                                app.cursor_position = app.input.len();
                            }
                            _ => {}
                        }
                    }
                    Event::Mouse(mouse) => {
                        match mouse.kind {
                            event::MouseEventKind::ScrollUp => {
                                app.scroll_up(3);
                            }
                            event::MouseEventKind::ScrollDown => {
                                app.scroll_down(3);
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            Some(chunk) = chunk_rx.recv() => {
                // Log the stream chunk
                if let Err(e) = app.logger.log_stream_chunk(&chunk) {
                    error!("Failed to log stream chunk: {}", e);
                }

                match chunk {
                    StreamChunk::Text { delta, done } => {
                        if done {
                            if app.expecting_tool_response {
                                // We just got a done after sending tool results, but don't finish streaming
                                // The actual response is coming
                                app.expecting_tool_response = false;
                            } else {
                                // Check if the chunk contains an error message
                                if delta.starts_with("Error") {
                                    app.finish_streaming();
                                    let error_msg = ChatMessage {
                                        role: MessageRole::System,
                                        content: delta.clone(),
                                        tool_call_id: None,
                                        tool_calls: None,
                                    };
                                    if let Err(e) = app.logger.log_message(&error_msg) {
                                        error!("Failed to log error message: {}", e);
                                    }
                                    app.messages.push(error_msg);
                                } else {
                                    app.finish_streaming();
                                }
                                app.scroll_to_bottom();
                            }
                        } else {
                            app.append_stream_chunk(&delta);
                            // Auto-scroll to bottom while streaming
                            app.scroll_to_bottom();
                            // Force UI redraw to ensure scroll takes effect
                            terminal.draw(|f| ui(f, app))?;
                        }
                    }
                    StreamChunk::ToolCall { id, name, arguments } => {
                        info!("=== TOOL CALL RECEIVED FROM SERVER ===");
                        info!("Tool ID: {}", id);
                        info!("Tool Name: {}", name);
                        info!("Arguments: {}", arguments);

                        // Display tool call as assistant message
                        // Try to pretty-print JSON arguments
                        let formatted_args = if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&arguments) {
                            serde_json::to_string_pretty(&json_val).unwrap_or_else(|_| arguments.to_string())
                        } else {
                            arguments.to_string()
                        };

                        let tool_call_msg = format!(
                            "🔧 Calling tool: {}\nArguments:\n{}",
                            name,
                            formatted_args
                        );
                        let tool_msg = ChatMessage {
                            role: MessageRole::System,
                            content: format!("[ToolInfo] {}", tool_call_msg),
                            tool_call_id: None,
                            tool_calls: None,
                        };
                        if let Err(e) = app.logger.log_message(&tool_msg) {
                            error!("Failed to log tool call message: {}", e);
                        }
                        app.messages.push(tool_msg);
                        app.scroll_to_bottom();

                        // Force UI refresh to show tool call immediately
                        terminal.draw(|f| ui(f, app))?;

                        // Execute the tool
                        let result = execute_tool(&name, &arguments).await;
                        info!("Tool execution completed");
                        info!("Result length: {} chars", result.len());
                        info!("Result preview (first 200 chars): {}",
                            if result.len() > 200 {
                                &result[..200]
                            } else {
                                &result
                            });

                        // Log tool execution result
                        if let Err(e) = app.logger.log_tool_execution(&id, &name, &result) {
                            error!("Failed to log tool execution: {}", e);
                        }

                        // Display tool result as assistant message
                        let result_msg = format!(
                            "📋 Tool result for {}:\n{}",
                            name,
                            if result.len() > 1000 {
                                format!("{}... (truncated, {} total chars)", &result[..1000], result.len())
                            } else {
                                result.clone()
                            }
                        );
                        let result_display_msg = ChatMessage {
                            role: MessageRole::System,
                            content: format!("[ToolInfo] {}", result_msg),
                            tool_call_id: None,
                            tool_calls: None,
                        };
                        if let Err(e) = app.logger.log_message(&result_display_msg) {
                            error!("Failed to log tool result display: {}", e);
                        }
                        app.messages.push(result_display_msg);
                        app.scroll_to_bottom();

                        // Force UI refresh to show tool result immediately
                        terminal.draw(|f| ui(f, app))?;

                        // Start streaming mode BEFORE sending tool result to avoid race condition
                        app.start_streaming();
                        app.expecting_tool_response = true;  // Mark that we're expecting a response after tool

                        // Send tool result back to server
                        info!("Sending tool result back to server...");
                        if let Err(e) = client.send_tool_result(id.clone(), result.clone()).await {
                            // If sending failed, cancel streaming mode
                            app.finish_streaming();
                            app.expecting_tool_response = false;
                            let error_msg = ChatMessage {
                                role: MessageRole::System,
                                content: format!("Failed to send tool result: {}", e),
                                tool_call_id: None,
                                tool_calls: None,
                            };
                            if let Err(log_err) = app.logger.log_message(&error_msg) {
                                error!("Failed to log error message: {}", log_err);
                            }
                            app.messages.push(error_msg);
                            info!("ERROR: Failed to send tool result: {}", e);
                        } else {
                            info!("Tool result successfully sent to server");
                            // Streaming mode already started, ready to receive response
                        }
                    }
                    StreamChunk::ToolResult { id, content } => {
                        // This shouldn't be received by the client from server
                        debug!("Unexpected tool result from server: {} - {}", id, content);
                    }
                }
            }
        }
    }
}

fn ui(f: &mut Frame, app: &App) {
    let area = f.area();
    
    // Split into main area and input area
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),     // Chat area takes remaining space
            Constraint::Length(3),  // Input box is always 3 lines
        ])
        .split(area);

    // Render chat messages
    render_chat(f, app, chunks[0]);
    
    // Render input box
    render_input(f, app, chunks[1]);
}

fn render_chat(f: &mut Frame, app: &App, area: Rect) {
    let mut all_lines: Vec<Line> = Vec::new();
    
    // Add connection status at the top
    let status_color = match &app.connection_status {
        ConnectionStatus::Connected => Color::Green,
        ConnectionStatus::Connecting => Color::Yellow,
        ConnectionStatus::Disconnected => Color::Red,
        ConnectionStatus::Error(_) => Color::Red,
    };
    
    let status_text = match &app.connection_status {
        ConnectionStatus::Connected => "● Connected",
        ConnectionStatus::Connecting => "● Connecting...",
        ConnectionStatus::Disconnected => "● Disconnected",
        ConnectionStatus::Error(e) => &format!("● Error: {}", e),
    };
    
    all_lines.push(Line::from(Span::styled(
        status_text,
        Style::default().fg(status_color),
    )));
    all_lines.push(Line::from(""));
    
    // Build all message lines
    let mut all_messages = app.messages.clone();
    if let Some(ref streaming) = app.streaming_message {
        all_messages.push(ChatMessage {
            role: MessageRole::Assistant,
            content: if streaming.is_empty() {
                "●●●".to_string()
            } else {
                format!("{}▌", streaming) // Add cursor to show it's still streaming
            },
            tool_call_id: None,
            tool_calls: None,
        });
    }
    
    for msg in &all_messages {
        let style = match msg.role {
            MessageRole::System => Style::default().fg(Color::Yellow),
            MessageRole::User => Style::default().fg(Color::Cyan),
            MessageRole::Assistant => Style::default().fg(Color::Green),
            MessageRole::Tool => Style::default().fg(Color::Magenta),
        };
        
        let prefix = match msg.role {
            MessageRole::System => "System",
            MessageRole::User => "You",
            MessageRole::Assistant => "Assistant",
            MessageRole::Tool => "Tool",
        };
        
        // Add role prefix
        all_lines.push(Line::from(Span::styled(
            format!("{}:", prefix),
            style.add_modifier(Modifier::BOLD),
        )));
        
        // Add message content lines
        for line in msg.content.lines() {
            all_lines.push(Line::from(Span::styled(line, style)));
        }
        
        // Add spacing after message
        all_lines.push(Line::from(""));
    }
    
    // Calculate visible lines based on scroll offset
    let total_lines = all_lines.len();
    let visible_height = area.height as usize;
    
    // Calculate the correct view window
    let start_line = if total_lines > visible_height {
        // If we have more lines than can fit
        let max_scroll = total_lines.saturating_sub(visible_height);
        let actual_scroll = app.scroll_offset.min(max_scroll);
        total_lines.saturating_sub(visible_height).saturating_sub(actual_scroll)
    } else {
        0
    };
    
    let end_line = (start_line + visible_height).min(total_lines);
    let visible_lines: Vec<Line> = all_lines[start_line..end_line].to_vec();
    
    // Create paragraph with visible lines
    let chat = Paragraph::new(visible_lines)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false });
    
    f.render_widget(chat, area);
}

fn render_input(f: &mut Frame, app: &App, area: Rect) {
    let input_text = if app.input.is_empty() {
        "Type your message..."
    } else {
        &app.input
    };
    
    let style = if app.input.is_empty() {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default()
    };
    
    let input = Paragraph::new(input_text)
        .style(style)
        .block(Block::default()
            .borders(Borders::ALL)
            .title("Input (Ctrl-Q to quit, ↑↓ to scroll)")
            .border_style(Style::default().fg(Color::White)))
        .wrap(Wrap { trim: true });
    
    f.render_widget(input, area);
    
    // Show cursor
    if !app.input.is_empty() {
        let cursor_x = area.x + app.cursor_position as u16 + 1;
        let cursor_y = area.y + 1;
        f.set_cursor_position((cursor_x.min(area.x + area.width - 2), cursor_y));
    }
}