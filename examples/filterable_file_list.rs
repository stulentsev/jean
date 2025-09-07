use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};
use std::{
    error::Error,
    io,
    path::PathBuf,
};
use walkdir::WalkDir;

struct App {
    input: String,
    cursor_position: usize,
    filtered_files: Vec<PathBuf>,
    all_files: Vec<PathBuf>,
    selected_index: usize,
}

impl App {
    fn new() -> Self {
        let all_files = Self::collect_all_files(".");
        Self {
            input: String::new(),
            cursor_position: 0,
            filtered_files: Vec::new(),
            all_files,
            selected_index: 0,
        }
    }

    fn collect_all_files(root: &str) -> Vec<PathBuf> {
        WalkDir::new(root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.path().to_path_buf())
            .collect()
    }

    fn get_current_search(&self) -> Option<(String, usize, usize)> {
        // Find @word pattern anywhere in input
        let words: Vec<&str> = self.input.split_whitespace().collect();
        
        for word in words {
            if let Some(search_str) = word.strip_prefix('@') {
                if !search_str.is_empty() {
                    // Find the position of this word in the original string
                    if let Some(pos) = self.input.find(word) {
                        return Some((search_str.to_string(), pos, pos + word.len()));
                    }
                }
            }
        }
        None
    }

    fn update_filter(&mut self) {
        if let Some((search_str, _, _)) = self.get_current_search() {
            self.filtered_files = self
                .all_files
                .iter()
                .filter(|path| {
                    path.to_string_lossy()
                        .to_lowercase()
                        .contains(&search_str.to_lowercase())
                })
                .take(5)
                .cloned()
                .collect();
            self.selected_index = 0;
        } else {
            self.filtered_files.clear();
        }
    }

    fn move_selection_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    fn move_selection_down(&mut self) {
        if self.selected_index < self.filtered_files.len().saturating_sub(1) {
            self.selected_index += 1;
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
        self.update_filter();
    }

    fn delete_char_before_cursor(&mut self) {
        if self.cursor_position > 0 {
            self.input.remove(self.cursor_position - 1);
            self.cursor_position -= 1;
            self.update_filter();
        }
    }

    fn get_all_references(&self) -> Vec<String> {
        self.input
            .split_whitespace()
            .filter_map(|word| {
                word.strip_prefix('@')
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
            })
            .collect()
    }

    fn create_styled_input(&self) -> Line<'_> {
        let mut spans = Vec::new();
        
        // Convert string to chars for proper indexing
        let chars: Vec<char> = self.input.chars().collect();
        
        // Build the spans with cursor
        let mut i = 0;
        while i < chars.len() || i == self.cursor_position {
            if i == self.cursor_position {
                // Render cursor
                let cursor_char = if i < chars.len() {
                    chars[i].to_string()
                } else {
                    " ".to_string()
                };
                spans.push(Span::styled(
                    cursor_char,
                    Style::default().bg(Color::White).fg(Color::Black),
                ));
                if i < chars.len() {
                    i += 1;
                } else {
                    break;
                }
            } else {
                // Find word boundaries for styling
                let start = i;
                while i < chars.len() && i != self.cursor_position && !chars[i].is_whitespace() {
                    i += 1;
                }
                
                if start < i {
                    let word: String = chars[start..i].iter().collect();
                    if word.starts_with('@') && word.len() > 1 {
                        spans.push(Span::styled(
                            word,
                            Style::default().fg(Color::Cyan).add_modifier(Modifier::UNDERLINED),
                        ));
                    } else {
                        spans.push(Span::raw(word));
                    }
                }
                
                // Handle whitespace
                if i < chars.len() && i != self.cursor_position && chars[i].is_whitespace() {
                    spans.push(Span::raw(chars[i].to_string()));
                    i += 1;
                }
            }
        }

        Line::from(spans)
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // create app and run it
    let app = App::new();
    let res = run_app(&mut terminal, app);

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err)
    }

    Ok(())
}

fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    mut app: App,
) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, &app))?;

        if let Event::Key(key) = event::read()? {
            // Only handle key press events, not key release
            if key.kind != KeyEventKind::Press {
                continue;
            }
            
            match key.code {
                KeyCode::Char('q') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                    return Ok(())
                }
                KeyCode::Char(c) => {
                    app.insert_char(c);
                }
                KeyCode::Backspace => {
                    app.delete_char_before_cursor();
                }
                KeyCode::Left => app.move_cursor_left(),
                KeyCode::Right => app.move_cursor_right(),
                KeyCode::Up => app.move_selection_up(),
                KeyCode::Down => app.move_selection_down(),
                KeyCode::Enter => {
                    if let Some(file) = app.filtered_files.get(app.selected_index) {
                        if let Some((_, start, end)) = app.get_current_search() {
                            if let Some(filename) = file.file_name() {
                                let filename_str = format!("@{}", filename.to_string_lossy());
                                app.input.replace_range(start..end, &filename_str);
                                app.cursor_position = start + filename_str.len();
                                app.update_filter();
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),    // Input field
            Constraint::Length(3),    // References display
            Constraint::Min(5),       // File list
            Constraint::Length(3),    // Status
        ].as_ref())
        .split(f.area());

    // Input field with styled references
    let input = Paragraph::new(app.create_styled_input())
        .block(Block::default().borders(Borders::ALL).title("Input (type @ to filter)"));
    f.render_widget(input, chunks[0]);

    // References display
    let references = app.get_all_references();
    let references_text = if references.is_empty() {
        "No references".to_string()
    } else {
        format!("References: {}", references.join(" "))
    };
    let references_widget = Paragraph::new(references_text)
        .style(Style::default().fg(Color::Green))
        .block(Block::default().borders(Borders::ALL).title("Attached Files"));
    f.render_widget(references_widget, chunks[1]);

    // File list
    if app.get_current_search().is_some() && !app.filtered_files.is_empty() {
        let items: Vec<ListItem> = app
            .filtered_files
            .iter()
            .enumerate()
            .map(|(i, path)| {
                let style = if i == app.selected_index {
                    Style::default()
                        .bg(Color::Blue)
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(path.display().to_string()).style(style)
            })
            .collect();

        let files = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Filtered Files"))
            .style(Style::default().fg(Color::White));

        f.render_widget(files, chunks[2]);
    }
    
    // Status line showing file count
    let status = format!(
        "Total files: {} | Filtered: {} | Search active: {}",
        app.all_files.len(),
        app.filtered_files.len(),
        app.get_current_search().is_some()
    );
    let status_widget = Paragraph::new(status)
        .style(Style::default().fg(Color::Gray))
        .block(Block::default().borders(Borders::ALL).title("Status"));
    f.render_widget(status_widget, chunks[3]);
}