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
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame, Terminal,
};
use speak_code_shared::{ChatMessage, ChatRequest, MessageRole, StreamChunk};
use std::io;
use tokio::sync::mpsc;

struct App {
    messages: Vec<ChatMessage>,
    input: String,
    scroll: u16,
    connection_status: ConnectionStatus,
    streaming_message: Option<String>,
}

impl App {
    fn new() -> Self {
        Self {
            messages: vec![ChatMessage {
                role: MessageRole::System,
                content: "Welcome to Speak Code! Connecting to backend...".to_string(),
            }],
            input: String::new(),
            scroll: 0,
            connection_status: ConnectionStatus::Disconnected,
            streaming_message: None,
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
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging to file instead of terminal to avoid corrupting TUI
    let log_file = std::fs::File::create("speak-code-cli.log").ok();
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
    let (client, mut chunk_rx) = BackendClient::new(ws_url);
    
    let (ui_tx, mut ui_rx) = mpsc::unbounded_channel();
    
    std::thread::spawn(move || {
        while let Ok(event) = event::read() {
            if ui_tx.send(event).is_err() {
                break;
            }
        }
    });
    
    let res = run_app(&mut terminal, &mut app, client, &mut chunk_rx, &mut ui_rx).await;

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
    ui_rx: &mut mpsc::UnboundedReceiver<Event>,
) -> Result<()> {
    loop {
        app.connection_status = client.get_status().await;
        terminal.draw(|f| ui(f, app))?;

        tokio::select! {
            Some(event) = ui_rx.recv() => {
                if let Event::Key(key) = event {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('q') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                                return Ok(())
                            }
                            KeyCode::Char(c) => {
                                app.input.push(c);
                            }
                            KeyCode::Backspace => {
                                app.input.pop();
                            }
                            KeyCode::Enter => {
                                if !app.input.is_empty() {
                                    let content = app.input.clone();
                                    app.add_user_message(content.clone());
                                    app.input.clear();
                                    
                                    // Filter out the initial system message that's just for UI
                                    let messages_to_send: Vec<ChatMessage> = app.messages
                                        .iter()
                                        .filter(|msg| {
                                            !(matches!(msg.role, MessageRole::System) && 
                                              msg.content.contains("Welcome to Speak Code"))
                                        })
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
                                if app.scroll < app.messages.len() as u16 {
                                    app.scroll += 1;
                                }
                            }
                            KeyCode::Down => {
                                if app.scroll > 0 {
                                    app.scroll -= 1;
                                }
                            }
                            _ => {}
                        }
                    }
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
                } else {
                    app.append_stream_chunk(&chunk.delta);
                }
            }
        }
    }
}

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(3),
        ])
        .split(f.area());

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
    
    let status = Paragraph::new(status_text)
        .style(Style::default().fg(status_color))
        .block(Block::default());
    f.render_widget(status, chunks[0]);

    let mut all_messages = app.messages.clone();
    if let Some(ref streaming) = app.streaming_message {
        all_messages.push(ChatMessage {
            role: MessageRole::Assistant,
            content: if streaming.is_empty() {
                "●●●".to_string()
            } else {
                streaming.clone()
            },
        });
    }

    let messages: Vec<ListItem> = all_messages
        .iter()
        .map(|msg| {
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
            
            // Split content by newlines to preserve formatting
            let mut lines = vec![
                Line::from(Span::styled(format!("{}:", prefix), style.add_modifier(Modifier::BOLD))),
            ];
            
            // Add each line of the message content
            for line in msg.content.lines() {
                lines.push(Line::from(Span::raw(line)));
            }
            
            // Add empty line after message
            lines.push(Line::from(""));
            
            ListItem::new(lines)
        })
        .collect();

    let messages_list = List::new(messages)
        .block(Block::default().borders(Borders::ALL).title("Chat"))
        .style(Style::default());

    f.render_widget(messages_list, chunks[1]);

    let input = Paragraph::new(app.input.as_str())
        .block(Block::default().borders(Borders::ALL).title("Input (Ctrl-Q to quit)"))
        .wrap(Wrap { trim: true });
    
    f.render_widget(input, chunks[2]);
}