mod client;

use anyhow::Result;
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
use jean_shared::{ChatMessage, ChatRequest, MessageRole, StreamChunk};
use std::io;
use tokio::sync::mpsc;

struct App {
    messages: Vec<ChatMessage>,
    input: String,
    scroll_offset: usize,
    connection_status: ConnectionStatus,
    streaming_message: Option<String>,
    cursor_position: usize,
}

impl App {
    fn new() -> Self {
        Self {
            messages: vec![],
            input: String::new(),
            scroll_offset: 0,
            connection_status: ConnectionStatus::Disconnected,
            streaming_message: None,
            cursor_position: 0,
        }
    }

    fn add_user_message(&mut self, content: String) {
        self.messages.push(ChatMessage {
            role: MessageRole::User,
            content,
        });
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
                self.messages.push(ChatMessage {
                    role: MessageRole::Assistant,
                    content,
                });
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
                                    
                                    // Filter out UI-only system messages
                                    let messages_to_send: Vec<ChatMessage> = app.messages
                                        .iter()
                                        .cloned()
                                        .collect();
                                    
                                    let request = ChatRequest {
                                        messages: messages_to_send,
                                        model: String::new(), // Server will use its configured model
                                        stream: true,
                                    };
                                    
                                    if let Err(e) = client.send_message(request).await {
                                        app.messages.push(ChatMessage {
                                            role: MessageRole::System,
                                            content: format!("Failed to send message: {}", e),
                                        });
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
                if chunk.done {
                    // Check if the chunk contains an error message
                    if chunk.delta.starts_with("Error") {
                        app.finish_streaming();
                        app.messages.push(ChatMessage {
                            role: MessageRole::System,
                            content: chunk.delta.clone(),
                        });
                    } else {
                        app.finish_streaming();
                    }
                    app.scroll_to_bottom();
                } else {
                    app.append_stream_chunk(&chunk.delta);
                    // Auto-scroll to bottom while streaming
                    app.scroll_to_bottom();
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
        });
    }
    
    for msg in &all_messages {
        let style = match msg.role {
            MessageRole::System => Style::default().fg(Color::Yellow),
            MessageRole::User => Style::default().fg(Color::Cyan),
            MessageRole::Assistant => Style::default().fg(Color::Green),
        };
        
        let prefix = match msg.role {
            MessageRole::System => "System",
            MessageRole::User => "You",
            MessageRole::Assistant => "Assistant",
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