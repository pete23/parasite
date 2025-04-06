use std::io;
use std::time::Duration;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

// Time adjustment constants in milliseconds
const NORMAL_TIME_ADJUST: i64 = 100;
const FINE_TIME_ADJUST: i64 = 25;

/// Parasite: Vocal Sample Pack Creator
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Directory containing VTT and WAV files
    #[arg(short, long, default_value = "data")]
    input_dir: String,

    /// Directory for saving extracted samples
    #[arg(short, long, default_value = "output")]
    output_dir: String,
}
use ratatui::{prelude::*, widgets::*};
use ratatui::widgets::{Row, Cell, Table, TableState};
use walkdir::WalkDir;
use thiserror::Error;

#[derive(Error, Debug)]
enum ParasiteError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    
    #[error("Audio processing error: {0}")]
    AudioProcessing(String),
}


struct App {
    vtt_files: Vec<PathBuf>,
    search_query: String,
    all_results: Vec<SearchResult>,     // All available results from files
    filtered_results: Vec<SearchResult>, // Results filtered by current search
    flat_results: Vec<DisplayLine>,     // Flattened results including context lines
    selected_idx: Option<usize>,        // Index in flat_results
    status_message: String,
    context_lines: usize,               // Number of context lines to include above/below matches
    input_dir: String,                  // Directory containing VTT and WAV files
    output_dir: String,                 // Directory for saving extracted samples
}

#[derive(Clone)]
struct SearchResult {
    file_path: PathBuf,
    text: String,
    start_time: Duration,
    end_time: Duration,
    context_before: Vec<(String, Duration, Duration)>,  // (Text, start_time, end_time) for context before
    context_after: Vec<(String, Duration, Duration)>,   // (Text, start_time, end_time) for context after
}

// A line that can be displayed and selected in the UI
#[derive(Clone)]
struct DisplayLine {
    text: String,         // Text content
    file_path: PathBuf,   // Source file
    start_time: Duration, // Start time for audio
    end_time: Duration,   // End time for audio
    is_match: bool,       // Whether this is a match (true) or context (false)
    original_start: Duration, // Original start time (for reference)
    original_end: Duration,   // Original end time (for reference)
}

impl App {
    fn new(input_dir: String, output_dir: String) -> Result<App> {
        // Load VTT files from input directory
        let vtt_files = WalkDir::new(&input_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "vtt"))
            .map(|e| e.path().to_path_buf())
            .collect::<Vec<_>>();
        
        let mut app = App {
            vtt_files,
            search_query: String::new(),
            all_results: Vec::new(),
            filtered_results: Vec::new(),
            flat_results: Vec::new(),
            selected_idx: None,
            status_message: String::from("Type to search, +/- for context, Tab to preview, Enter to extract"),
            context_lines: 0, // Start with no context lines
            input_dir,
            output_dir,
        };
        
        app.load_all_results()?;
        app.filter_results();
        
        // Update the status message to include directory information
        app.status_message = format!("Loaded {} samples from {}. Saving to {}.", 
                                    app.all_results.len(), 
                                    app.input_dir,
                                    app.output_dir);
        
        Ok(app)
    }
    
    // Adjust the start time of the selected line
    fn adjust_start_time(&mut self, delta_ms: i64) {
        if let Some(idx) = self.selected_idx {
            // First, compute the new start time value
            let new_start_time = if idx < self.flat_results.len() {
                let line = &self.flat_results[idx];
                
                // Calculate new timestamp ensuring it doesn't go negative
                let current_ms = line.start_time.as_millis() as i64;
                
                // If trying to decrease below zero, just set to zero and notify
                if current_ms + delta_ms < 0 {
                    self.status_message = "Start time already at minimum (0)".to_string();
                    return;
                }
                
                let new_ms = current_ms + delta_ms;
                
                // Ensure start time doesn't go beyond end time
                let end_ms = line.end_time.as_millis() as i64;
                
                // If end time is too close or less than new start time, don't allow the adjustment
                if end_ms <= new_ms + 10 {
                    self.status_message = "Cannot adjust: Start time would exceed end time".to_string();
                    return;
                }
                
                // Create new duration with the safe value
                Duration::from_millis(new_ms as u64)
            } else {
                return; // Invalid index
            };
            
            // Get the original start time for status message
            let original_start = self.flat_results[idx].original_start;
            
            // Apply the new time to current segment only
            self.flat_results[idx].start_time = new_start_time;
            
            // We no longer automatically adjust the previous segment's end time
            // This prevents cascading timing issues
            
            // Update status message showing adjustment
            let delta_sign = if delta_ms >= 0 { "+" } else { "-" };
            let original_to_now = new_start_time.as_millis() as i64 - original_start.as_millis() as i64;
            let net_sign = if original_to_now >= 0 { "+" } else { "-" };
            
            self.status_message = format!(
                "Start time adjusted by {}{}ms ({}{}ms from original)",
                delta_sign, delta_ms.abs(), net_sign, original_to_now.abs()
            );
        }
    }
    
    // Adjust the end time of the selected line
    fn adjust_end_time(&mut self, delta_ms: i64) {
        if let Some(idx) = self.selected_idx {
            // First, compute the new end time value
            let new_end_time = if idx < self.flat_results.len() {
                let line = &self.flat_results[idx];
                
                // Calculate new timestamp
                let current_ms = line.end_time.as_millis() as i64;
                
                // For end time, we need to determine the maximum duration
                // Get the next segment's start time as a limit, if available
                let max_end_ms = if idx < self.flat_results.len() - 1 {
                    // If there's a next segment, use its start time as the maximum
                    self.flat_results[idx + 1].start_time.as_millis() as i64
                } else {
                    // If there's no next segment, use a reasonable maximum
                    // (current time + 30 seconds should be enough for most use cases)
                    current_ms + 30_000
                };
                
                // If trying to decrease below start time + minimum gap, prevent it
                let start_ms = line.start_time.as_millis() as i64;
                
                if current_ms + delta_ms < start_ms + 10 {
                    self.status_message = "Cannot adjust: End time would precede start time".to_string();
                    return;
                }
                
                // If trying to increase beyond maximum, prevent it
                if current_ms + delta_ms > max_end_ms {
                    self.status_message = format!("Cannot adjust: End time would exceed maximum ({:.2}s)", 
                                                max_end_ms as f64 / 1000.0);
                    return;
                }
                
                // Add the delta and ensure we stay within bounds
                let new_ms = std::cmp::max(start_ms + 10, 
                             std::cmp::min(max_end_ms, current_ms + delta_ms));
                
                // Create new duration
                Duration::from_millis(new_ms as u64)
            } else {
                return; // Invalid index
            };
            
            // Get the original end time for status message
            let original_end = self.flat_results[idx].original_end;
            
            // Apply the new time to current segment only
            self.flat_results[idx].end_time = new_end_time;
            
            // We no longer automatically adjust the next segment's start time
            // This prevents cascading timing issues
            
            // Update status message showing adjustment
            let delta_sign = if delta_ms >= 0 { "+" } else { "-" };
            let original_to_now = new_end_time.as_millis() as i64 - original_end.as_millis() as i64;
            let net_sign = if original_to_now >= 0 { "+" } else { "-" };
            
            self.status_message = format!(
                "End time adjusted by {}{}ms ({}{}ms from original)",
                delta_sign, delta_ms.abs(), net_sign, original_to_now.abs()
            );
        }
    }
    
    fn load_all_results(&mut self) -> Result<()> {
        self.all_results.clear();
        
        for file_path in &self.vtt_files {
            let content = std::fs::read_to_string(file_path)?;
            
            // Basic VTT parsing
            let lines: Vec<&str> = content.lines().collect();
            
            for i in 0..lines.len() {
                // Skip WEBVTT header and timing lines
                if i > 0 && !lines[i].contains("-->") && !lines[i].trim().is_empty() {
                    let text = lines[i].trim();
                    
                    // Find timing info from previous line
                    if i > 0 {
                        if let Some(timing_line) = lines[0..i].iter().rev().find(|line| line.contains("-->")) {
                            if let Some((start_time, end_time)) = parse_time_range(timing_line) {
                                // Collect context lines (text lines, not timing lines)
                                let mut context_before = Vec::new();
                                let mut context_after = Vec::new();
                                
                                // Look for context before (up to MAX_CONTEXT_LINES)
                                let mut before_idx = i as i32 - 1;
                                while before_idx >= 0 && context_before.len() < 5 {
                                    let before_line = lines[before_idx as usize].trim();
                                    if !before_line.contains("-->") && !before_line.is_empty() {
                                        // Find timing for this context line
                                        if let Some(context_timing) = lines[0..before_idx as usize]
                                            .iter()
                                            .rev()
                                            .find(|line| line.contains("-->")) {
                                            if let Some((ctx_start, ctx_end)) = parse_time_range(context_timing) {
                                                context_before.insert(0, (before_line.to_string(), ctx_start, ctx_end));
                                            }
                                        }
                                    }
                                    before_idx -= 1;
                                }
                                
                                // Look for context after (up to MAX_CONTEXT_LINES)
                                let mut after_idx = i + 1;
                                while after_idx < lines.len() && context_after.len() < 5 {
                                    let after_line = lines[after_idx].trim();
                                    if !after_line.contains("-->") && !after_line.is_empty() {
                                        // Find timing for this context line
                                        if let Some(context_timing) = lines[0..after_idx]
                                            .iter()
                                            .rev()
                                            .find(|line| line.contains("-->")) {
                                            if let Some((ctx_start, ctx_end)) = parse_time_range(context_timing) {
                                                context_after.push((after_line.to_string(), ctx_start, ctx_end));
                                            }
                                        }
                                    }
                                    after_idx += 1;
                                }
                                
                                self.all_results.push(SearchResult {
                                    file_path: file_path.clone(),
                                    text: text.to_string(),
                                    start_time,
                                    end_time,
                                    context_before,
                                    context_after,
                                });
                            }
                        }
                    }
                }
            }
        }
        
        self.status_message = format!("Loaded {} samples", self.all_results.len());
        Ok(())
    }
    
    fn filter_results(&mut self) {
        if self.search_query.is_empty() {
            // Show all results when no search query
            self.filtered_results = self.all_results.clone();
        } else {
            // Split search query into individual words
            let search_words: Vec<&str> = self.search_query
                .split_whitespace()
                .collect();
            
            // Filter results to include only those containing all search words
            self.filtered_results = self.all_results
                .iter()
                .filter(|result| {
                    let text_lower = result.text.to_lowercase();
                    // Check if all words in the search query appear in the text
                    search_words.iter().all(|word| {
                        text_lower.contains(&word.to_lowercase())
                    })
                })
                .cloned()
                .collect();
        }
        
        // Create flat list of results with context
        self.flatten_results();
        
        // Update selection to first item if available
        self.selected_idx = if self.flat_results.is_empty() { 
            None 
        } else {
            Some(0)
        };
        
        self.status_message = format!("Found {} matches, {} total lines with context", 
                                    self.filtered_results.len(), self.flat_results.len());
    }
    
    // Helper function to check if two segments are approximately the same
    fn is_same_segment(text1: &str, start1: &Duration, end1: &Duration,
                       text2: &str, start2: &Duration, end2: &Duration) -> bool {
        // First check if text matches
        if text1 != text2 {
            return false;
        }
        
        // Compare durations with tolerance - Duration doesn't have abs()
        let start_diff = if *start1 > *start2 {
            *start1 - *start2
        } else {
            *start2 - *start1
        };
        
        let end_diff = if *end1 > *end2 {
            *end1 - *end2
        } else {
            *end2 - *end1
        };
        
        // Consider it a match if timings are within 10ms
        start_diff < Duration::from_millis(10) && 
        end_diff < Duration::from_millis(10)
    }
    
    // Create a flat list of display lines including context
    fn flatten_results(&mut self) {
        self.flat_results.clear();
        
        // Track which text segments are actual matches to avoid duplicating them as context
        let match_segments: Vec<(String, Duration, Duration)> = self.filtered_results
            .iter()
            .map(|r| (r.text.clone(), r.start_time, r.end_time))
            .collect();
        
        for result in &self.filtered_results {
            // Add context before if enabled
            if self.context_lines > 0 {
                for (i, (ctx_text, ctx_start, ctx_end)) in result.context_before.iter()
                    .rev()  // Reverse to get the most recent first
                    .take(self.context_lines)
                    .enumerate() {
                    
                    // Skip if this context line is already a match elsewhere
                    let is_also_match = match_segments.iter().any(|(match_text, match_start, match_end)| {
                        // Use our helper function to compare segments
                        Self::is_same_segment(match_text, match_start, match_end, 
                                             ctx_text, ctx_start, ctx_end)
                    });
                    
                    if !is_also_match {
                        // Add context lines in original order
                        let ctx_idx = result.context_before.len() - 1 - i;
                        if ctx_idx < result.context_before.len() {
                            self.flat_results.push(DisplayLine {
                                text: format!("↑ {}", ctx_text),
                                file_path: result.file_path.clone(),
                                start_time: *ctx_start,
                                end_time: *ctx_end,
                                is_match: false, // This is context, not a match
                                original_start: *ctx_start,
                                original_end: *ctx_end,
                            });
                        }
                    }
                }
            }
            
            // Add the main result line
            self.flat_results.push(DisplayLine {
                text: result.text.clone(),
                file_path: result.file_path.clone(),
                start_time: result.start_time,
                end_time: result.end_time,
                is_match: true, // This is a match
                original_start: result.start_time, // Store original values
                original_end: result.end_time,
            });
            
            // Add context after if enabled
            if self.context_lines > 0 {
                for (ctx_text, ctx_start, ctx_end) in result.context_after.iter()
                    .take(self.context_lines) {
                    
                    // Skip if this context line is already a match elsewhere
                    let is_also_match = match_segments.iter().any(|(match_text, match_start, match_end)| {
                        // Use our helper function to compare segments (same as for context_before)
                        Self::is_same_segment(match_text, match_start, match_end, 
                                             ctx_text, ctx_start, ctx_end)
                    });
                    
                    if !is_also_match {
                        self.flat_results.push(DisplayLine {
                            text: format!("↓ {}", ctx_text),
                            file_path: result.file_path.clone(),
                            start_time: *ctx_start,
                            end_time: *ctx_end,
                            is_match: false, // This is context, not a match
                            original_start: *ctx_start,
                            original_end: *ctx_end,
                        });
                    }
                }
            }
        }
    }
    
    // Extract a sample from any line in flat_results
    fn extract_flat_line(&self, idx: usize) -> Result<String> {
        if let Some(line) = self.flat_results.get(idx) {
            // Get corresponding wav file path
            let wav_path = line.file_path.with_extension("wav");
            
            if !wav_path.exists() {
                return Err(ParasiteError::AudioProcessing(format!("WAV file not found: {:?}", wav_path)).into());
            }
            
            // Generate output filename based on selected text (first few words)
            let text_words: Vec<_> = line.text.split_whitespace().take(3).collect();
            let output_name = text_words.join("_").to_lowercase();
            let output_path = PathBuf::from(format!("{}/{}.wav", self.output_dir, output_name));
            
            // Ensure we have a valid duration (start before end)
            if line.end_time <= line.start_time {
                return Err(ParasiteError::AudioProcessing("Invalid time range: end time must be after start time".to_string()).into());
            }
            
            // Use ffmpeg to extract the segment with full timestamp precision
            let output = Command::new("ffmpeg")
                .args([
                    "-i", &wav_path.to_string_lossy(),
                    "-ss", &format!("{}", line.start_time.as_secs_f64()),
                    "-t", &format!("{}", (line.end_time - line.start_time).as_secs_f64()),
                    "-c:a", "copy",
                    &output_path.to_string_lossy(),
                    "-y" // Overwrite if exists
                ])
                .output()?;
        
            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(ParasiteError::AudioProcessing(format!("ffmpeg error: {}", error)).into());
            }
            
            return Ok(output_name);
        }
        
        Err(ParasiteError::AudioProcessing("No line selected".to_string()).into())
    }
    
    
    // Preview any line (match or context) from the flat list
    fn preview_flat_line(&self, idx: usize) -> Result<()> {
        if let Some(line) = self.flat_results.get(idx) {
            // Get corresponding wav file path
            let wav_path = line.file_path.with_extension("wav");
            
            if !wav_path.exists() {
                return Err(ParasiteError::AudioProcessing(format!("WAV file not found: {:?}", wav_path)).into());
            }
            
            // Ensure we have a valid duration (start before end)
            if line.end_time <= line.start_time {
                return Err(ParasiteError::AudioProcessing("Invalid time range: end time must be after start time".to_string()).into());
            }
            
            // Use ffplay to play the segment with full timestamp precision
            
            // Use ffplay with -nodisp to not show video window, and -autoexit to exit after playback
            // Redirect stdout and stderr to /dev/null to prevent TUI disruption
            let _child = Command::new("ffplay")
                .args([
                    "-nodisp",
                    "-autoexit",
                    "-loglevel", "quiet",  // Suppress all output
                    "-ss", &format!("{}", line.start_time.as_secs_f64()),
                    "-t", &format!("{}", (line.end_time - line.start_time).as_secs_f64()),
                    &wav_path.to_string_lossy(),
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()?;
            
            // Note: We don't wait for the child process to finish to keep the UI responsive
            
            return Ok(());
        }
        
        Err(ParasiteError::AudioProcessing("No line selected".to_string()).into())
    }
    
}

fn parse_time_range(line: &str) -> Option<(Duration, Duration)> {
    let parts: Vec<&str> = line.split("-->").collect();
    if parts.len() != 2 {
        return None;
    }
    
    let start = parse_timestamp(parts[0].trim())?;
    let end = parse_timestamp(parts[1].trim())?;
    
    Some((start, end))
}

fn parse_timestamp(timestamp: &str) -> Option<Duration> {
    let parts: Vec<&str> = timestamp.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    
    let hours: u64 = parts[0].trim().parse().ok()?;
    let minutes: u64 = parts[1].trim().parse().ok()?;
    
    let seconds_parts: Vec<&str> = parts[2].split('.').collect();
    if seconds_parts.len() != 2 {
        return None;
    }
    
    let seconds: u64 = seconds_parts[0].trim().parse().ok()?;
    let milliseconds: u64 = seconds_parts[1].trim().parse().ok()?;
    
    let total_millis = hours * 3600000 + minutes * 60000 + seconds * 1000 + milliseconds;
    Some(Duration::from_millis(total_millis))
}

fn ui(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),  // Status and search bar (increased height)
            Constraint::Min(0),     // Main content
            Constraint::Length(1),  // Help
        ])
        .split(frame.size());

    // Status bar and search input
    let search_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // Status message
            Constraint::Length(3),  // Search input (increased height)
        ])
        .split(chunks[0]);
    
    // Status message
    frame.render_widget(
        Paragraph::new(app.status_message.clone())
            .style(Style::default().fg(Color::Cyan)),
        search_area[0],
    );
    
    // Very simple search input that should definitely work
    let query_display = if app.search_query.is_empty() {
        "Type to search...".to_string()
    } else {
        format!("Search: {}", app.search_query)
    };
    
    let search_input = Paragraph::new(query_display)
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Left)
        .block(Block::default().borders(Borders::ALL).title("Search"));
    
    frame.render_widget(search_input, search_area[1]);

    // Create a table for results
    let selected_style = Style::default()
        .bg(Color::DarkGray)
        .add_modifier(Modifier::BOLD);
    
    // Create the table rows
    let rows: Vec<Row> = app.flat_results
        .iter()
        .map(|line| {
            // Get and truncate filename to 30 chars
            let filename = line.file_path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            let truncated_filename = if filename.len() > 30 {
                format!("{}...", &filename[0..27])
            } else {
                filename.to_string()
            };
            
            // Format start time
            let start_time = format!(
                "{:>8}",
                format!("{}.{:03}", 
                    line.start_time.as_secs() % 60,
                    line.start_time.subsec_millis()
                )
            );
            
            // Format end time
            let end_time = format!(
                "{:>8}",
                format!("{}.{:03}", 
                    line.end_time.as_secs() % 60,
                    line.end_time.subsec_millis()
                )
            );
            
            // Calculate duration (safely handle case where end might be before start)
            let duration = if line.end_time > line.start_time {
                format!("{:.2}s", (line.end_time - line.start_time).as_secs_f64())
            } else {
                "0.00s".to_string() // Handle invalid duration case
            };
            
            // Format text with prefix for context lines
            let text = line.text.clone();
            
            // Set style based on whether it's a match or context
            let style = if line.is_match {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            
            Row::new(vec![
                Cell::from(truncated_filename).style(style),
                Cell::from(start_time).style(style),
                Cell::from(end_time).style(style),
                Cell::from(duration).style(style),
                Cell::from(text).style(style),
            ])
        })
        .collect();
    
    // Create column widths
    let widths = [
        Constraint::Length(30), // File name
        Constraint::Length(10), // Start time
        Constraint::Length(10), // End time
        Constraint::Length(8),  // Duration
        Constraint::Percentage(100), // Text (remaining space)
    ];
    
    // Create the table
    let table = Table::new(rows, widths)
        .header(Row::new(vec![
            Cell::from("File").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("Start").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("End").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("Length").style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from("Text").style(Style::default().add_modifier(Modifier::BOLD)),
        ]))
        .block(Block::default()
            .borders(Borders::ALL)
            .title(format!("Results ({} matches, {} total lines)", 
                  app.filtered_results.len(), app.flat_results.len())))
        .highlight_style(selected_style)
        .highlight_symbol("> ");
    
    let mut list_state = TableState::default();
    list_state.select(app.selected_idx);
    
    frame.render_stateful_widget(table, chunks[1], &mut list_state);

    // Help text including context controls
    let context_help = format!("Context: {} lines", app.context_lines);
    frame.render_widget(
        Paragraph::new(format!("Type to search | +/-: context ({}) | ,/./[/]: adjust time | </>/{{/}}: fine adjust | Esc: reset time | Tab: preview | Enter: extract | q: quit", context_help))
            .alignment(Alignment::Center),
        chunks[2],
    );
}

fn run_app(input_dir: String, output_dir: String) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    
    // Create app state
    let mut app = App::new(input_dir, output_dir)?;
    
    loop {
        terminal.draw(|f| ui(f, &app))?;
        
        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('+') => {
                            // Increase context lines (max 5)
                            if app.context_lines < 5 {
                                app.context_lines += 1;
                                // Recreate the flat list with new context amount
                                app.flatten_results();
                                app.status_message = format!("Context set to {} lines", app.context_lines);
                            } else {
                                app.status_message = "Maximum context lines reached (5)".to_string();
                            }
                        },
                        KeyCode::Char('-') => {
                            // Decrease context lines (min 0)
                            if app.context_lines > 0 {
                                app.context_lines -= 1;
                                // Recreate the flat list with new context amount
                                app.flatten_results();
                                app.status_message = format!("Context set to {} lines", app.context_lines);
                            } else {
                                app.status_message = "Context lines already at minimum (0)".to_string();
                            }
                        },
                        // Handle timing adjustment keys and handle their shifted variants
                        KeyCode::Char('<') | KeyCode::Char('>') | KeyCode::Char('[') | KeyCode::Char(']') |
                        KeyCode::Char(',') | KeyCode::Char('.') | KeyCode::Char('{') | KeyCode::Char('}') => {
                            // Determine which key was pressed (including the Shift variants)
                            let (adjustment_direction, adjust_start) = match key.code {
                                // Start time adjustments
                                KeyCode::Char('<') | KeyCode::Char(',') => (-1, true),  // Decrease start time
                                KeyCode::Char('>') | KeyCode::Char('.') => (1, true),   // Increase start time
                                
                                // End time adjustments
                                KeyCode::Char('[') | KeyCode::Char('{') => (-1, false), // Decrease end time
                                KeyCode::Char(']') | KeyCode::Char('}') => (1, false),  // Increase end time
                                
                                _ => unreachable!(), // This case can't happen due to the match condition
                            };
                            
                            // Determine adjustment magnitude
                            // Small adjustments for shifted symbols (<>{}), large for unshifted (,.[])
                            let adjustment_value = match key.code {
                                KeyCode::Char('<') | KeyCode::Char('>') | 
                                KeyCode::Char('{') | KeyCode::Char('}') => FINE_TIME_ADJUST,
                                _ => NORMAL_TIME_ADJUST,
                            };
                            
                            // Apply the adjustment
                            if adjust_start {
                                app.adjust_start_time(adjustment_direction * adjustment_value);
                            } else {
                                app.adjust_end_time(adjustment_direction * adjustment_value);
                            }
                        },
                        KeyCode::Esc => {
                            // Reset timestamps to original values (previously 'c')
                            if let Some(idx) = app.selected_idx {
                                if idx < app.flat_results.len() {
                                    // Get the original values
                                    let original_start = app.flat_results[idx].original_start;
                                    let original_end = app.flat_results[idx].original_end;
                                    
                                    // Reset to original values
                                    app.flat_results[idx].start_time = original_start;
                                    app.flat_results[idx].end_time = original_end;
                                    
                                    app.status_message = "Timestamps reset to original values.".to_string();
                                }
                            } else {
                                app.status_message = "No line selected".to_string();
                            }
                        },
                        KeyCode::Tab => {
                            // Preview the selected line (match or context) (previously 'p')
                            if let Some(idx) = app.selected_idx {
                                match app.preview_flat_line(idx) {
                                    Ok(_) => {
                                        let line = &app.flat_results[idx];
                                        let duration_secs = (line.end_time - line.start_time).as_secs_f64();
                                        let line_type = if line.is_match { "match" } else { "context" };
                                        app.status_message = format!(
                                            "Preview playing ({}): \"{}\" ({:.2}s)",
                                            line_type,
                                            line.text,
                                            duration_secs
                                        );
                                    }
                                    Err(e) => app.status_message = format!("Preview error: {}", e),
                                }
                            } else {
                                app.status_message = "No line selected".to_string();
                            }
                        }
                        KeyCode::Char(c) => {
                            app.search_query.push(c);
                            app.filter_results();
                        }
                        KeyCode::Backspace => {
                            app.search_query.pop();
                            app.filter_results();
                        }
                        KeyCode::Enter => {
                            // Extract sample on Enter from any line (match or context)
                            if let Some(idx) = app.selected_idx {
                                match app.extract_flat_line(idx) {
                                    Ok(sample_name) => {
                                        let line = &app.flat_results[idx];
                                        let duration_secs = (line.end_time - line.start_time).as_secs_f64();
                                        let line_type = if line.is_match { "match" } else { "context" };
                                        app.status_message = format!(
                                            "Sample saved: {}/{}.wav ({}, {:.2}s)",
                                            app.output_dir,
                                            sample_name,
                                            line_type,
                                            duration_secs
                                        );
                                    }
                                    Err(e) => app.status_message = format!("Error: {}", e),
                                }
                            } else {
                                app.status_message = "No line selected".to_string();
                            }
                        }
                        KeyCode::Up => {
                            app.selected_idx = match app.selected_idx {
                                Some(i) if i > 0 => Some(i - 1),
                                Some(i) => Some(i),
                                None if !app.flat_results.is_empty() => Some(0),
                                None => None,
                            };
                        }
                        KeyCode::Down => {
                            app.selected_idx = match app.selected_idx {
                                Some(i) if i + 1 < app.flat_results.len() => Some(i + 1),
                                Some(i) => Some(i),
                                None if !app.flat_results.is_empty() => Some(0),
                                None => None,
                            };
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    
    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    
    Ok(())
}

fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();
    
    // Check if input directory exists
    if !std::path::Path::new(&args.input_dir).exists() {
        println!("Warning: Input directory '{}' does not exist. Creating it...", args.input_dir);
        std::fs::create_dir(&args.input_dir)?;
        println!("Please place your VTT and WAV files in the '{}' directory.", args.input_dir);
    }
    
    // Create output directory if it doesn't exist
    if !std::path::Path::new(&args.output_dir).exists() {
        std::fs::create_dir(&args.output_dir)?;
    }
    
    // Run the application
    if let Err(err) = run_app(args.input_dir, args.output_dir) {
        eprintln!("Error: {}", err);
    }
    
    Ok(())
}