use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame, Terminal,
};
use speak_code_shared::{ChatMessage, MessageRole};
use std::io;

struct App {
    messages: Vec<ChatMessage>,
    input: String,
    scroll: u16,
}

impl App {
    fn new() -> Self {
        Self {
            messages: vec![ChatMessage {
                role: MessageRole::System,
                content: "Welcome to Speak Code! Type your message and press Enter.".to_string(),
            }],
            input: String::new(),
            scroll: 0,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let res = run_app(&mut terminal, &mut app).await;

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

async fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        if let Event::Key(key) = event::read()? {
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
                            let msg = ChatMessage {
                                role: MessageRole::User,
                                content: app.input.clone(),
                            };
                            app.messages.push(msg);
                            app.input.clear();

                            // TODO: Send to backend
                            app.messages.push(ChatMessage {
                                role: MessageRole::Assistant,
                                content: "I'm not connected to the backend yet!".to_string(),
                            });
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
}

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(f.area());

    let messages: Vec<ListItem> = app
        .messages
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
            
            ListItem::new(vec![
                Line::from(Span::styled(format!("{}:", prefix), style.add_modifier(Modifier::BOLD))),
                Line::from(Span::raw(&msg.content)),
                Line::from(""),
            ])
        })
        .collect();

    let messages_list = List::new(messages)
        .block(Block::default().borders(Borders::ALL).title("Chat"))
        .style(Style::default());

    f.render_widget(messages_list, chunks[0]);

    let input = Paragraph::new(app.input.as_str())
        .block(Block::default().borders(Borders::ALL).title("Input (Ctrl-Q to quit)"))
        .wrap(Wrap { trim: true });
    
    f.render_widget(input, chunks[1]);
}