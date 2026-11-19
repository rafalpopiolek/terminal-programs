//! Scrollable pager state, vim-like key handling and drawing.

use std::num::NonZeroU16;

use ratatui::Frame;
use ratatui::buffer::{Buffer, CellDiffOption};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

use unicode_width::UnicodeWidthStr;

use crate::markdown::{self, LinkArea};

const MAX_CONTENT_WIDTH: u16 = 100;

const STATUS_BG: Color = Color::Rgb(40, 44, 52);
const ACCENT: Color = Color::Rgb(198, 120, 221);
const MATCH_STYLE: Style = Style::new().fg(Color::Black).bg(Color::Rgb(229, 192, 123));
const CURRENT_MATCH_STYLE: Style = Style::new().fg(Color::Black).bg(Color::Rgb(209, 154, 102));

const HELP: &[(&str, &str)] = &[
    ("j / k / ↓ / ↑", "line down / up"),
    ("d / u, Ctrl-d / Ctrl-u", "half page down / up"),
    ("Space / b, Ctrl-f / Ctrl-b", "page down / up"),
    ("gg / G", "top / bottom"),
    ("5j, 10G ...", "repeat count / go to line"),
    ("] / [", "next / previous heading"),
    ("/ text Enter", "search"),
    ("n / N", "next / previous match"),
    ("Esc", "clear search highlight"),
    ("Cmd/Ctrl + click", "open link"),
    ("r", "reload file"),
    ("q", "quit"),
];

pub enum Outcome {
    Continue,
    Quit,
    Reload,
}

enum Mode {
    Normal,
    SearchInput(String),
    Help,
}

pub struct Viewer {
    title: String,
    source: String,
    lines: Vec<Line<'static>>,
    searchable: Vec<Vec<char>>,
    headings: Vec<usize>,
    links: Vec<LinkArea>,
    render_width: usize,
    page_height: usize,
    scroll: usize,
    mode: Mode,
    query: Vec<char>,
    matches: Vec<usize>,
    current_match: Option<usize>,
    count: Option<usize>,
    pending_g: bool,
    message: Option<String>,
}

impl Viewer {
    pub fn new(title: String, source: String) -> Self {
        Self {
            title,
            source,
            lines: Vec::new(),
            searchable: Vec::new(),
            headings: Vec::new(),
            links: Vec::new(),
            render_width: 0,
            page_height: 1,
            scroll: 0,
            mode: Mode::Normal,
            query: Vec::new(),
            matches: Vec::new(),
            current_match: None,
            count: None,
            pending_g: false,
            message: None,
        }
    }

    pub fn reload(&mut self, source: String) {
        self.source = source;
        self.rerender(self.render_width);
        self.message = Some("reloaded".into());
    }

    pub fn show_error(&mut self, message: String) {
        self.message = Some(message);
    }

    fn rerender(&mut self, width: usize) {
        let document = markdown::render(&self.source, width);
        self.searchable = document.lines.iter().map(|line| searchable_chars(line)).collect();
        self.lines = document.lines;
        self.headings = document.headings;
        self.links = document.links;
        self.render_width = width;
        self.find_matches();
        self.clamp_scroll();
    }

    fn resize(&mut self, width: usize, height: usize) {
        self.page_height = height.max(1);
        if width != self.render_width {
            let position = self.scroll as f64 / self.lines.len().max(1) as f64;
            self.rerender(width);
            self.scroll = (position * self.lines.len() as f64).round() as usize;
        }
        self.clamp_scroll();
    }

    fn max_scroll(&self) -> usize {
        self.lines.len().saturating_sub(self.page_height)
    }

    fn clamp_scroll(&mut self) {
        self.scroll = self.scroll.min(self.max_scroll());
    }

    fn scroll_down(&mut self, lines: usize) {
        self.scroll = self.scroll.saturating_add(lines);
        self.clamp_scroll();
    }

    fn scroll_up(&mut self, lines: usize) {
        self.scroll = self.scroll.saturating_sub(lines);
    }

    fn scroll_to(&mut self, line: usize) {
        self.scroll = line;
        self.clamp_scroll();
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Outcome {
        self.message = None;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('c') {
            return Outcome::Quit;
        }
        match &mut self.mode {
            Mode::Normal => self.handle_normal_key(key.code, ctrl),
            Mode::Help => {
                self.mode = Mode::Normal;
                Outcome::Continue
            }
            Mode::SearchInput(input) => {
                match key.code {
                    KeyCode::Esc => self.mode = Mode::Normal,
                    KeyCode::Enter => {
                        let input: Vec<char> = input.chars().collect();
                        self.mode = Mode::Normal;
                        self.search(input);
                    }
                    KeyCode::Backspace => {
                        if input.pop().is_none() {
                            self.mode = Mode::Normal;
                        }
                    }
                    KeyCode::Char(c) if !ctrl => input.push(c),
                    _ => {}
                }
                Outcome::Continue
            }
        }
    }

    fn handle_normal_key(&mut self, code: KeyCode, ctrl: bool) -> Outcome {
        if let KeyCode::Char(digit @ '0'..='9') = code
            && !ctrl
            && (digit != '0' || self.count.is_some())
        {
            let value = digit.to_digit(10).unwrap_or(0) as usize;
            self.count = Some(self.count.unwrap_or(0).saturating_mul(10).saturating_add(value));
            return Outcome::Continue;
        }
        let count = self.count.take();
        let repeat = count.unwrap_or(1);
        let pending_g = std::mem::take(&mut self.pending_g);
        let half_page = (self.page_height / 2).max(1);
        let full_page = self.page_height.saturating_sub(2).max(1);

        match (code, ctrl) {
            (KeyCode::Char('q'), false) => return Outcome::Quit,
            (KeyCode::Char('r'), false) => return Outcome::Reload,
            (KeyCode::Char('j') | KeyCode::Down | KeyCode::Enter, _)
            | (KeyCode::Char('e') | KeyCode::Char('n'), true) => self.scroll_down(repeat),
            (KeyCode::Char('k') | KeyCode::Up, _) | (KeyCode::Char('y') | KeyCode::Char('p'), true) => {
                self.scroll_up(repeat)
            }
            (KeyCode::Char('d'), _) => self.scroll_down(half_page * repeat),
            (KeyCode::Char('u'), _) => self.scroll_up(half_page * repeat),
            (KeyCode::Char('f'), _) | (KeyCode::Char(' ') | KeyCode::PageDown, false) => {
                self.scroll_down(full_page * repeat)
            }
            (KeyCode::Char('b'), _) | (KeyCode::PageUp, false) => self.scroll_up(full_page * repeat),
            (KeyCode::Char('g'), false) if pending_g => self.scroll_to(count.unwrap_or(1) - 1),
            (KeyCode::Char('g'), false) => {
                self.pending_g = true;
                self.count = count;
            }
            (KeyCode::Home, _) => self.scroll_to(0),
            (KeyCode::Char('G'), false) => match count {
                Some(line) => self.scroll_to(line.saturating_sub(1)),
                None => self.scroll_to(usize::MAX),
            },
            (KeyCode::End, _) => self.scroll_to(usize::MAX),
            (KeyCode::Char(']' | '}'), false) => self.next_heading(),
            (KeyCode::Char('[' | '{'), false) => self.previous_heading(),
            (KeyCode::Char('/'), false) => self.mode = Mode::SearchInput(String::new()),
            (KeyCode::Char('n'), false) => self.jump_to_match(true),
            (KeyCode::Char('N'), false) => self.jump_to_match(false),
            (KeyCode::Esc, _) => {
                self.query.clear();
                self.matches.clear();
                self.current_match = None;
            }
            (KeyCode::Char('?'), false) => self.mode = Mode::Help,
            _ => {}
        }
        Outcome::Continue
    }

    fn next_heading(&mut self) {
        if let Some(&line) = self.headings.iter().find(|&&line| line > self.scroll) {
            self.scroll_to(line);
        }
    }

    fn previous_heading(&mut self) {
        if let Some(&line) = self.headings.iter().rev().find(|&&line| line < self.scroll) {
            self.scroll_to(line);
        }
    }

    fn search(&mut self, input: Vec<char>) {
        if !input.is_empty() {
            self.query = input.iter().map(|&c| lowercase(c)).collect();
        }
        if self.query.is_empty() {
            return;
        }
        self.find_matches();
        if self.matches.is_empty() {
            let query: String = self.query.iter().collect();
            self.message = Some(format!("Pattern not found: {query}"));
            return;
        }
        let first_visible = self.matches.iter().position(|&line| line >= self.scroll).unwrap_or(0);
        self.select_match(first_visible);
    }

    fn find_matches(&mut self) {
        self.current_match = None;
        self.matches = if self.query.is_empty() {
            Vec::new()
        } else {
            (0..self.searchable.len())
                .filter(|&index| !match_positions(&self.searchable[index], &self.query).is_empty())
                .collect()
        };
    }

    fn jump_to_match(&mut self, forward: bool) {
        if self.matches.is_empty() {
            if !self.query.is_empty() {
                self.message = Some("Pattern not found".into());
            }
            return;
        }
        let count = self.matches.len();
        let next = match self.current_match {
            Some(current) if forward => (current + 1) % count,
            Some(current) => (current + count - 1) % count,
            None => 0,
        };
        if forward && self.current_match == Some(count - 1) || !forward && self.current_match == Some(0) {
            self.message = Some("search wrapped".into());
        }
        self.select_match(next);
    }

    fn select_match(&mut self, index: usize) {
        self.current_match = Some(index);
        let line = self.matches[index];
        let visible = line >= self.scroll && line < self.scroll + self.page_height;
        if !visible {
            self.scroll_to(line.saturating_sub(self.page_height / 3));
        }
        self.message = Some(format!("match {}/{}", index + 1, self.matches.len()));
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let [body, status] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());
        let content_width = body.width.saturating_sub(4).clamp(1, MAX_CONTENT_WIDTH);
        let content = Rect {
            x: body.x + (body.width - content_width) / 2,
            width: content_width,
            ..body
        };
        self.resize(content_width as usize, body.height as usize);

        let end = (self.scroll + self.page_height).min(self.lines.len());
        let current_line = self.current_match.map(|index| self.matches[index]);
        let visible: Vec<Line> = (self.scroll..end)
            .map(|index| {
                if self.query.is_empty() {
                    return self.lines[index].clone();
                }
                let style = if Some(index) == current_line { CURRENT_MATCH_STYLE } else { MATCH_STYLE };
                highlight_matches(&self.lines[index], &self.searchable[index], &self.query, style)
            })
            .collect();
        frame.render_widget(Paragraph::new(visible), content);
        self.mark_hyperlinks(frame.buffer_mut(), content);

        if self.max_scroll() > 0 {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some(" "))
                .thumb_style(Style::new().fg(Color::Rgb(92, 99, 112)));
            let mut state = ScrollbarState::new(self.max_scroll()).position(self.scroll);
            frame.render_stateful_widget(scrollbar, body, &mut state);
        }

        self.draw_status(frame, status);

        if matches!(self.mode, Mode::Help) {
            draw_help(frame, body);
        }
    }

    /// Wraps visible link text in OSC 8 sequences, so the terminal opens the URL on Cmd/Ctrl+click.
    fn mark_hyperlinks(&self, buffer: &mut Buffer, area: Rect) {
        let visible_lines = self.scroll..self.scroll + area.height as usize;
        for link in self.links.iter().filter(|link| visible_lines.contains(&link.line)) {
            let url: String = link.url.chars().filter(|c| !c.is_control()).collect();
            let y = area.y + (link.line - self.scroll) as u16;
            let mut column = link.columns.start;
            while column < link.columns.end.min(area.width as usize) {
                let cell = &mut buffer[(area.x + column as u16, y)];
                let symbol = cell.symbol().to_string();
                let width = symbol.width().max(1);
                cell.set_symbol(&format!("\x1b]8;;{url}\x1b\\{symbol}\x1b]8;;\x1b\\"));
                cell.set_diff_option(CellDiffOption::ForcedWidth(NonZeroU16::new(width as u16).unwrap_or(NonZeroU16::MIN)));
                column += width;
            }
        }
    }

    fn draw_status(&self, frame: &mut Frame, area: Rect) {
        let base = Style::new().bg(STATUS_BG).fg(Color::Rgb(171, 178, 191));
        let left = match &self.mode {
            Mode::SearchInput(input) => Line::from(vec![
                Span::raw(format!(" /{input}")),
                Span::styled(" ", Style::new().bg(ACCENT)),
            ]),
            _ => {
                let mut spans = vec![Span::styled(
                    format!(" {} ", self.title),
                    Style::new().fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD),
                )];
                if let Some(message) = &self.message {
                    spans.push(Span::raw(format!("  {message}")));
                }
                Line::from(spans)
            }
        };

        let total = self.lines.len();
        let last_visible = (self.scroll + self.page_height).min(total);
        let position = if self.max_scroll() == 0 {
            "All".to_string()
        } else if self.scroll == 0 {
            "Top".to_string()
        } else if self.scroll >= self.max_scroll() {
            "Bot".to_string()
        } else {
            format!("{}%", self.scroll * 100 / self.max_scroll())
        };
        let pending = self.count.map(|n| n.to_string()).unwrap_or_default() + if self.pending_g { "g" } else { "" };
        let right = format!("{pending}  {}-{last_visible}/{total}  {position:>4}   ? help ", self.scroll + 1);

        frame.render_widget(Paragraph::new(left).style(base), area);
        frame.render_widget(Paragraph::new(right).right_aligned(), area);
    }
}

fn searchable_chars(line: &Line) -> Vec<char> {
    line.spans.iter().flat_map(|span| span.content.chars()).map(lowercase).collect()
}

fn lowercase(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Char indexes covered by any occurrence of `query` in `haystack`.
fn match_positions(haystack: &[char], query: &[char]) -> Vec<usize> {
    if query.is_empty() || query.len() > haystack.len() {
        return Vec::new();
    }
    let mut positions = Vec::new();
    let mut start = 0;
    while start + query.len() <= haystack.len() {
        if haystack[start..start + query.len()] == *query {
            positions.extend(start..start + query.len());
            start += query.len();
        } else {
            start += 1;
        }
    }
    positions
}

fn highlight_matches(line: &Line<'static>, searchable: &[char], query: &[char], style: Style) -> Line<'static> {
    let positions = match_positions(searchable, query);
    if positions.is_empty() {
        return line.clone();
    }
    let mut highlighted = vec![false; searchable.len()];
    for position in positions {
        highlighted[position] = true;
    }
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut index = 0;
    for span in &line.spans {
        for ch in span.content.chars() {
            let ch_style = if highlighted[index] { span.style.patch(style) } else { span.style };
            match spans.last_mut() {
                Some(last) if last.style == ch_style => last.content.to_mut().push(ch),
                _ => spans.push(Span::styled(ch.to_string(), ch_style)),
            }
            index += 1;
        }
    }
    Line::from(spans)
}

fn draw_help(frame: &mut Frame, area: Rect) {
    let key_width = HELP.iter().map(|(key, _)| key.chars().count()).max().unwrap_or(0);
    let lines: Vec<Line> = HELP
        .iter()
        .map(|(key, description)| {
            Line::from(vec![
                Span::styled(format!(" {key:<key_width$}  "), Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)),
                Span::raw(*description),
            ])
        })
        .collect();
    let width = (lines.iter().map(Line::width).max().unwrap_or(0) as u16 + 3).min(area.width);
    let height = (lines.len() as u16 + 2).min(area.height);
    let popup = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT))
        .title(" keys ");
    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;

    const WIDTH: u16 = 44;
    const HEIGHT: u16 = 6;

    fn numbered_lines(count: usize) -> String {
        (1..=count).map(|n| format!("line {n}\n\n")).collect()
    }

    fn press(sut: &mut Viewer, keys: &str) {
        for ch in keys.chars() {
            let code = if ch == '\n' { KeyCode::Enter } else { KeyCode::Char(ch) };
            sut.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        }
    }

    fn ctrl(sut: &mut Viewer, ch: char) {
        sut.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL));
    }

    /// Text visible in the content area, one entry per row.
    fn screen(sut: &mut Viewer) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap();
        terminal.draw(|frame| sut.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..HEIGHT - 1)
            .map(|y| {
                let row: String = (0..WIDTH - 1).map(|x| buffer[(x, y)].symbol()).collect();
                row.trim().to_string()
            })
            .collect()
    }

    fn viewer(source: String) -> Viewer {
        let mut sut = Viewer::new("test.md".into(), source);
        screen(&mut sut);
        sut
    }

    #[test]
    fn document_is_shown_from_the_top() {
        let mut sut = viewer(numbered_lines(10));

        assert_eq!(screen(&mut sut), vec!["line 1", "", "line 2", "", "line 3"]);
    }

    #[test]
    fn moving_down_with_a_count_skips_that_many_lines() {
        let mut sut = viewer(numbered_lines(10));

        press(&mut sut, "4j");

        assert_eq!(screen(&mut sut)[0], "line 3");
    }

    #[test]
    fn scrolling_stops_at_the_last_page() {
        let mut sut = viewer(numbered_lines(10));

        press(&mut sut, "500j");

        assert_eq!(screen(&mut sut).last().unwrap(), "line 10");
    }

    #[test]
    fn going_to_bottom_and_back_to_top() {
        let mut sut = viewer(numbered_lines(10));

        press(&mut sut, "G");
        let bottom = screen(&mut sut);
        press(&mut sut, "gg");

        assert_eq!(bottom.last().unwrap(), "line 10");
        assert_eq!(screen(&mut sut)[0], "line 1");
    }

    #[test]
    fn half_page_down_and_up_returns_to_the_same_place() {
        let mut sut = viewer(numbered_lines(10));

        ctrl(&mut sut, 'd');
        let moved = screen(&mut sut)[0].clone();
        ctrl(&mut sut, 'u');

        assert_ne!(moved, "line 1");
        assert_eq!(screen(&mut sut)[0], "line 1");
    }

    #[test]
    fn search_brings_the_match_into_view() {
        let mut sut = viewer(numbered_lines(30) + "the needle is here\n");

        press(&mut sut, "/NEEDLE\n");

        assert!(screen(&mut sut).contains(&"the needle is here".to_string()));
    }

    #[test]
    fn next_match_moves_through_all_occurrences() {
        let mut sut = viewer(format!("alpha\n\n{}alpha again\n", numbered_lines(20)));

        press(&mut sut, "/alpha\n");
        let first = screen(&mut sut);
        press(&mut sut, "n");

        assert!(first.contains(&"alpha".to_string()));
        assert!(screen(&mut sut).contains(&"alpha again".to_string()));
    }

    #[test]
    fn heading_jump_puts_the_next_heading_at_the_top() {
        let mut sut = viewer(format!("# Intro\n\n{0}### Details\n\n{0}", numbered_lines(10)));

        press(&mut sut, "]");

        assert_eq!(screen(&mut sut)[0], "Details");
    }

    /// Visible characters that the terminal receives wrapped in an OSC 8 hyperlink to `url`.
    fn hyperlinked_text(sut: &mut Viewer, url: &str) -> String {
        let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap();
        terminal.draw(|frame| sut.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let opening = format!("\x1b]8;;{url}\x1b\\");
        buffer
            .content()
            .iter()
            .filter_map(|cell| cell.symbol().strip_prefix(&opening))
            .map(|rest| rest.trim_end_matches("\x1b]8;;\x1b\\").to_string())
            .collect()
    }

    #[test]
    fn address_is_sent_to_the_terminal_as_a_clickable_hyperlink() {
        let mut sut = viewer("Open https://example.com/a/very/long/path/that/wraps today".into());

        assert_eq!(hyperlinked_text(&mut sut, "https://example.com/a/very/long/path/that/wraps"), "https://example.com/a/very/long/path/that/wraps");
    }

    #[test]
    fn hyperlinks_follow_the_text_when_scrolling() {
        let mut sut = viewer(numbered_lines(5) + "https://example.com\n");

        let before = hyperlinked_text(&mut sut, "https://example.com");
        press(&mut sut, "G");

        assert_eq!(before, "");
        assert_eq!(hyperlinked_text(&mut sut, "https://example.com"), "https://example.com");
    }
}
