//! Converts Markdown source into styled, pre-wrapped terminal lines.

use std::sync::OnceLock;

use pulldown_cmark::{
    Alignment, BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd,
};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const PURPLE: Color = Color::Rgb(198, 120, 221);
const BLUE: Color = Color::Rgb(97, 175, 239);
const CYAN: Color = Color::Rgb(86, 182, 194);
const GREEN: Color = Color::Rgb(152, 195, 121);
const YELLOW: Color = Color::Rgb(229, 192, 123);
const ORANGE: Color = Color::Rgb(209, 154, 102);
const RED: Color = Color::Rgb(224, 108, 117);
const GRAY: Color = Color::Rgb(92, 99, 112);
const LIGHT_GRAY: Color = Color::Rgb(171, 178, 191);
const CODE_BG: Color = Color::Rgb(43, 48, 59);

const MIN_WIDTH: usize = 20;
const SYNTAX_THEME: &str = "base16-ocean.dark";

/// Markdown rendered for a fixed width.
pub struct Document {
    pub lines: Vec<Line<'static>>,
    /// Indexes of lines where headings start.
    pub headings: Vec<usize>,
}

pub fn render(markdown: &str, width: usize) -> Document {
    let mut renderer = Renderer::new(width.max(MIN_WIDTH));
    for event in Parser::new_ext(markdown, parser_options()) {
        renderer.event(event);
    }
    renderer.finish()
}

fn parser_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_GFM
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
}

enum Container {
    Quote(Color),
    Item { marker: Span<'static>, used: bool },
}

struct CodeBlock {
    lang: String,
    text: String,
}

struct Table {
    alignments: Vec<Alignment>,
    rows: Vec<Vec<Vec<Span<'static>>>>,
    current_row: Vec<Vec<Span<'static>>>,
    in_head: bool,
    header_rows: usize,
}

struct Renderer {
    width: usize,
    lines: Vec<Line<'static>>,
    headings: Vec<usize>,
    inline: Vec<Span<'static>>,
    styles: Vec<Style>,
    containers: Vec<Container>,
    lists: Vec<Option<u64>>,
    links: Vec<(String, usize)>,
    heading: Option<HeadingLevel>,
    code: Option<CodeBlock>,
    table: Option<Table>,
    in_metadata: bool,
    last_blank: bool,
    at_block_start: bool,
}

impl Renderer {
    fn new(width: usize) -> Self {
        Self {
            width,
            lines: Vec::new(),
            headings: Vec::new(),
            inline: Vec::new(),
            styles: Vec::new(),
            containers: Vec::new(),
            lists: Vec::new(),
            links: Vec::new(),
            heading: None,
            code: None,
            table: None,
            in_metadata: false,
            last_blank: false,
            at_block_start: false,
        }
    }

    fn finish(mut self) -> Document {
        self.flush();
        while self.lines.last().is_some_and(|line| line.width() == 0) {
            self.lines.pop();
        }
        Document {
            lines: self.lines,
            headings: self.headings,
        }
    }

    fn event(&mut self, event: Event) {
        if self.in_metadata {
            self.in_metadata = !matches!(event, Event::End(TagEnd::MetadataBlock(_)));
            return;
        }
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => match &mut self.code {
                Some(code) => code.text.push_str(&text),
                None => self.push_text(&text),
            },
            Event::Code(text) => {
                let style = self.style().fg(YELLOW).bg(CODE_BG);
                self.inline.push(Span::styled(text.into_string(), style));
            }
            Event::SoftBreak => self.push_text(" "),
            Event::HardBreak => self.push_text("\n"),
            Event::Html(html) => self.html_block(&html),
            Event::InlineHtml(html) => self.inline_html(&html),
            Event::FootnoteReference(label) => {
                let style = Style::new().fg(CYAN);
                self.inline.push(Span::styled(format!("[{label}]"), style));
            }
            Event::Rule => {
                self.flush();
                self.gap();
                let prefix = self.rest_prefix();
                let rule = "─".repeat(self.width.saturating_sub(spans_width(&prefix)));
                self.push_line(line_with_prefix(prefix, vec![Span::styled(rule, Style::new().fg(GRAY))]));
            }
            Event::TaskListMarker(done) => self.set_task_marker(done),
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {
                self.flush();
                self.gap();
            }
            Tag::Heading { level, .. } => {
                self.flush();
                self.gap();
                self.heading = Some(level);
                self.styles.push(heading_style(level));
            }
            Tag::BlockQuote(kind) => self.start_quote(kind),
            Tag::CodeBlock(kind) => {
                self.flush();
                self.gap();
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or("").to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some(CodeBlock { lang, text: String::new() });
            }
            Tag::HtmlBlock => {
                self.flush();
                self.gap();
            }
            Tag::List(start) => {
                self.flush();
                if !self.in_list_item() {
                    self.gap();
                }
                self.lists.push(start);
            }
            Tag::Item => {
                self.flush();
                let marker = self.next_list_marker();
                self.containers.push(Container::Item { marker, used: false });
            }
            Tag::FootnoteDefinition(label) => {
                self.flush();
                self.gap();
                let marker = Span::styled(format!("[{label}] "), Style::new().fg(CYAN));
                self.containers.push(Container::Item { marker, used: false });
            }
            Tag::Table(alignments) => {
                self.flush();
                self.gap();
                self.table = Some(Table {
                    alignments,
                    rows: Vec::new(),
                    current_row: Vec::new(),
                    in_head: false,
                    header_rows: 0,
                });
            }
            Tag::TableHead => {
                if let Some(table) = &mut self.table {
                    table.in_head = true;
                }
            }
            Tag::Emphasis => self.styles.push(Style::new().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.styles.push(Style::new().add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => self.styles.push(Style::new().add_modifier(Modifier::CROSSED_OUT)),
            Tag::Link { dest_url, .. } => {
                self.styles.push(Style::new().fg(BLUE).add_modifier(Modifier::UNDERLINED));
                self.links.push((dest_url.into_string(), self.inline.len()));
            }
            Tag::Image { .. } => {
                let style = Style::new().fg(GREEN).add_modifier(Modifier::ITALIC);
                self.inline.push(Span::styled("▣ ", style));
                self.styles.push(style);
            }
            Tag::MetadataBlock(_) => self.in_metadata = true,
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.flush(),
            TagEnd::Heading(level) => {
                self.flush_heading(level);
                self.styles.pop();
                self.heading = None;
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.containers.pop();
                self.styles.pop();
            }
            TagEnd::CodeBlock => {
                if let Some(code) = self.code.take() {
                    self.render_code(code);
                }
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
            }
            TagEnd::Item | TagEnd::FootnoteDefinition => {
                self.flush();
                self.containers.pop();
            }
            TagEnd::TableCell => {
                let mut cell = std::mem::take(&mut self.inline);
                if let Some(table) = &mut self.table {
                    if table.in_head {
                        for span in &mut cell {
                            span.style = span.style.add_modifier(Modifier::BOLD);
                        }
                    }
                    table.current_row.push(cell);
                }
            }
            TagEnd::TableHead | TagEnd::TableRow => {
                if let Some(table) = &mut self.table {
                    let row = std::mem::take(&mut table.current_row);
                    table.rows.push(row);
                    if table.in_head {
                        table.in_head = false;
                        table.header_rows = table.rows.len();
                    }
                }
            }
            TagEnd::Table => {
                if let Some(table) = self.table.take() {
                    self.render_table(table);
                }
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Image => {
                self.styles.pop();
            }
            TagEnd::Link => {
                self.styles.pop();
                if let Some((url, start)) = self.links.pop() {
                    let text: String = self.inline[start..].iter().map(|s| s.content.as_ref()).collect();
                    if !url.is_empty() && !url.starts_with('#') && text != url {
                        self.inline.push(Span::styled(format!(" ({url})"), Style::new().fg(GRAY)));
                    }
                }
            }
            _ => {}
        }
    }

    fn style(&self) -> Style {
        self.styles.iter().fold(Style::new(), |acc, style| acc.patch(*style))
    }

    fn push_text(&mut self, text: &str) {
        let style = self.style();
        self.inline.push(Span::styled(text.to_string(), style));
    }

    fn in_list_item(&self) -> bool {
        self.containers.iter().any(|c| matches!(c, Container::Item { .. }))
    }

    fn next_list_marker(&mut self) -> Span<'static> {
        let depth = self.lists.len().saturating_sub(1);
        let style = Style::new().fg(ORANGE);
        match self.lists.last_mut() {
            Some(Some(number)) => {
                let marker = format!("{number}. ");
                *number += 1;
                Span::styled(marker, style)
            }
            _ => {
                let bullet = ["•", "◦", "▪"][depth % 3];
                Span::styled(format!("{bullet} "), style)
            }
        }
    }

    fn set_task_marker(&mut self, done: bool) {
        let (symbol, color) = if done { ("☑ ", GREEN) } else { ("☐ ", LIGHT_GRAY) };
        let unused_item = self.containers.iter_mut().rev().find_map(|c| match c {
            Container::Item { marker, used: false } => Some(marker),
            _ => None,
        });
        if let Some(marker) = unused_item {
            *marker = Span::styled(symbol, Style::new().fg(color));
        }
    }

    fn start_quote(&mut self, kind: Option<BlockQuoteKind>) {
        self.flush();
        self.gap();
        let alert = kind.map(|kind| match kind {
            BlockQuoteKind::Note => ("ⓘ Note", BLUE),
            BlockQuoteKind::Tip => ("✦ Tip", GREEN),
            BlockQuoteKind::Important => ("❢ Important", PURPLE),
            BlockQuoteKind::Warning => ("⚠ Warning", YELLOW),
            BlockQuoteKind::Caution => ("⛔ Caution", RED),
        });
        let color = alert.map_or(GRAY, |(_, color)| color);
        self.containers.push(Container::Quote(color));
        self.styles.push(Style::new().fg(LIGHT_GRAY).add_modifier(Modifier::ITALIC));
        if let Some((title, color)) = alert {
            let prefix = self.rest_prefix();
            let title = Span::styled(title, Style::new().fg(color).add_modifier(Modifier::BOLD));
            self.push_line(line_with_prefix(prefix, vec![title]));
        }
        self.at_block_start = true;
    }

    /// Prefix for the first line of a block (list markers shown once) and for its continuation lines.
    fn prefixes(&mut self) -> (Vec<Span<'static>>, Vec<Span<'static>>) {
        let mut first = Vec::new();
        let mut rest = Vec::new();
        for container in &mut self.containers {
            match container {
                Container::Quote(color) => {
                    let bar = Span::styled("│ ", Style::new().fg(*color));
                    first.push(bar.clone());
                    rest.push(bar);
                }
                Container::Item { marker, used } => {
                    let indent = " ".repeat(marker.width());
                    first.push(if *used { Span::raw(indent.clone()) } else { marker.clone() });
                    rest.push(Span::raw(indent));
                    *used = true;
                }
            }
        }
        (first, rest)
    }

    fn rest_prefix(&self) -> Vec<Span<'static>> {
        self.containers
            .iter()
            .map(|container| match container {
                Container::Quote(color) => Span::styled("│ ", Style::new().fg(*color)),
                Container::Item { marker, .. } => Span::raw(" ".repeat(marker.width())),
            })
            .collect()
    }

    fn push_line(&mut self, line: Line<'static>) {
        self.lines.push(line);
        self.last_blank = false;
        self.at_block_start = false;
    }

    /// Separates consecutive blocks with one blank line (keeping quote bars).
    fn gap(&mut self) {
        if self.lines.is_empty() || self.last_blank || self.at_block_start {
            return;
        }
        let mut prefix = self.rest_prefix();
        while prefix.last().is_some_and(|span| span.content.trim().is_empty()) {
            prefix.pop();
        }
        if let Some(last) = prefix.last_mut() {
            last.content = last.content.trim_end().to_string().into();
        }
        self.lines.push(Line::from(prefix));
        self.last_blank = true;
    }

    fn flush(&mut self) {
        if self.inline.iter().all(|span| span.content.trim().is_empty()) {
            self.inline.clear();
            return;
        }
        let spans = std::mem::take(&mut self.inline);
        let (first, rest) = self.prefixes();
        for line in wrap(&spans, self.width, &first, &rest) {
            self.push_line(line);
        }
    }

    fn flush_heading(&mut self, level: HeadingLevel) {
        let (first, rest) = self.prefixes();
        let spans = std::mem::take(&mut self.inline);
        let lines = wrap(&spans, self.width, &first, &rest);
        if lines.is_empty() {
            return;
        }
        self.headings.push(self.lines.len());
        let prefix_width = spans_width(&rest);
        let text_width = lines.iter().map(|l| l.width().saturating_sub(prefix_width)).max().unwrap_or(0);
        for line in lines {
            self.push_line(line);
        }
        let underline = match level {
            HeadingLevel::H1 => Some(("━", PURPLE)),
            HeadingLevel::H2 => Some(("─", BLUE)),
            _ => None,
        };
        if let Some((symbol, color)) = underline {
            let rule = Span::styled(symbol.repeat(text_width), Style::new().fg(color));
            self.push_line(line_with_prefix(rest, vec![rule]));
        }
    }

    fn html_block(&mut self, html: &str) {
        if html.trim_start().starts_with("<!--") {
            return;
        }
        for text in html.lines().filter(|l| !l.trim().is_empty()) {
            self.inline.push(Span::styled(text.trim().to_string(), Style::new().fg(GRAY)));
            self.inline.push(Span::raw("\n"));
        }
        self.flush();
    }

    fn inline_html(&mut self, html: &str) {
        let tag = html.trim().to_ascii_lowercase();
        if tag.starts_with("<br") {
            self.push_text("\n");
        } else if !tag.starts_with("<!--") {
            self.inline.push(Span::styled(html.to_string(), Style::new().fg(GRAY)));
        }
    }

    fn render_code(&mut self, code: CodeBlock) {
        let (first, rest) = self.prefixes();
        let block_width = self.width.saturating_sub(spans_width(&rest)).max(MIN_WIDTH);
        let code_width = block_width.saturating_sub(4).max(1);
        let background = Style::new().bg(CODE_BG);

        let mut rows: Vec<Vec<Span<'static>>> = Vec::new();
        let label = Span::styled(code.lang.clone(), background.fg(GRAY).add_modifier(Modifier::ITALIC));
        let label_pad = block_width.saturating_sub(label.width() + 1);
        rows.push(vec![Span::styled(" ".repeat(label_pad), background), label, Span::styled(" ", background)]);
        for source_line in highlight(&code.text, &code.lang) {
            for chunk in hard_wrap(&source_line, code_width) {
                let mut row = vec![Span::styled("  ", background)];
                let used = 2 + spans_width(&chunk);
                row.extend(chunk.into_iter().map(|span| {
                    let style = background.patch(span.style);
                    span.style(style)
                }));
                row.push(Span::styled(" ".repeat(block_width.saturating_sub(used)), background));
                rows.push(row);
            }
        }
        rows.push(vec![Span::styled(" ".repeat(block_width), background)]);

        for (index, row) in rows.into_iter().enumerate() {
            let prefix = if index == 0 { first.clone() } else { rest.clone() };
            self.push_line(line_with_prefix(prefix, row));
        }
    }

    fn render_table(&mut self, table: Table) {
        let (first, rest) = self.prefixes();
        let border = Style::new().fg(GRAY);
        let columns = table.rows.iter().map(Vec::len).max().unwrap_or(0);
        if columns == 0 {
            return;
        }
        let mut widths = vec![1; columns];
        for row in &table.rows {
            for (col, cell) in row.iter().enumerate() {
                widths[col] = widths[col].max(spans_width(cell));
            }
        }
        let available = self.width.saturating_sub(spans_width(&rest) + 3 * columns + 1);
        while widths.iter().sum::<usize>() > available {
            let widest = (0..columns).max_by_key(|&c| widths[c]).unwrap_or(0);
            if widths[widest] <= 3 {
                break;
            }
            widths[widest] -= 1;
        }

        let rule = |left: &str, middle: &str, right: &str| {
            let parts: Vec<String> = widths.iter().map(|w| "─".repeat(w + 2)).collect();
            vec![Span::styled(format!("{left}{}{right}", parts.join(middle)), border)]
        };

        let mut rows = vec![rule("┌", "┬", "┐")];
        for (index, row) in table.rows.iter().enumerate() {
            if index > 0 && index == table.header_rows {
                rows.push(rule("├", "┼", "┤"));
            }
            let cells: Vec<Vec<Line<'static>>> = (0..columns)
                .map(|col| match row.get(col) {
                    Some(cell) => wrap(cell, widths[col], &[], &[]),
                    None => Vec::new(),
                })
                .collect();
            let height = cells.iter().map(Vec::len).max().unwrap_or(0).max(1);
            for line_index in 0..height {
                let mut spans = vec![Span::styled("│", border)];
                for (col, cell) in cells.iter().enumerate() {
                    let content = cell.get(line_index).cloned().unwrap_or_default();
                    let alignment = table.alignments.get(col).copied().unwrap_or(Alignment::None);
                    spans.push(Span::raw(" "));
                    spans.extend(align(content, widths[col], alignment));
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled("│", border));
                }
                rows.push(spans);
            }
        }
        rows.push(rule("└", "┴", "┘"));

        for (index, row) in rows.into_iter().enumerate() {
            let prefix = if index == 0 { first.clone() } else { rest.clone() };
            self.push_line(line_with_prefix(prefix, row));
        }
    }
}

fn heading_style(level: HeadingLevel) -> Style {
    let bold = Style::new().add_modifier(Modifier::BOLD);
    match level {
        HeadingLevel::H1 => bold.fg(PURPLE),
        HeadingLevel::H2 => bold.fg(BLUE),
        HeadingLevel::H3 => bold.fg(CYAN),
        HeadingLevel::H4 => bold.fg(GREEN),
        HeadingLevel::H5 => bold.fg(YELLOW),
        HeadingLevel::H6 => bold.fg(LIGHT_GRAY).add_modifier(Modifier::ITALIC),
    }
}

fn align(line: Line<'static>, width: usize, alignment: Alignment) -> Vec<Span<'static>> {
    let free = width.saturating_sub(line.width());
    let (left, right) = match alignment {
        Alignment::Right => (free, 0),
        Alignment::Center => (free / 2, free - free / 2),
        Alignment::Left | Alignment::None => (0, free),
    };
    let mut spans = vec![Span::raw(" ".repeat(left))];
    spans.extend(line.spans);
    spans.push(Span::raw(" ".repeat(right)));
    spans
}

fn line_with_prefix(mut prefix: Vec<Span<'static>>, content: Vec<Span<'static>>) -> Line<'static> {
    prefix.extend(content);
    Line::from(prefix)
}

fn spans_width(spans: &[Span]) -> usize {
    spans.iter().map(Span::width).sum()
}

fn push_merged(spans: &mut Vec<Span<'static>>, text: &str, style: Style) {
    match spans.last_mut() {
        Some(last) if last.style == style => last.content.to_mut().push_str(text),
        _ => spans.push(Span::styled(text.to_string(), style)),
    }
}

enum Token {
    /// Pieces of one word that must not be split (e.g. `**bold**,` spans two styles).
    Word(Vec<(String, Style)>),
    Space(Style),
    Newline,
}

fn tokenize(spans: &[Span<'static>]) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut word: Vec<(String, Style)> = Vec::new();
    for span in spans {
        for ch in span.content.chars() {
            if ch == '\n' || ch.is_whitespace() {
                if !word.is_empty() {
                    tokens.push(Token::Word(std::mem::take(&mut word)));
                }
                tokens.push(if ch == '\n' { Token::Newline } else { Token::Space(span.style) });
                continue;
            }
            match word.last_mut() {
                Some((text, style)) if *style == span.style => text.push(ch),
                _ => word.push((ch.to_string(), span.style)),
            }
        }
    }
    if !word.is_empty() {
        tokens.push(Token::Word(word));
    }
    tokens
}

/// Greedy word wrap of styled text into lines no wider than `width` (prefix included).
pub fn wrap(
    spans: &[Span<'static>],
    width: usize,
    first_prefix: &[Span<'static>],
    rest_prefix: &[Span<'static>],
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut current: Vec<Span<'static>> = first_prefix.to_vec();
    let mut prefix_width = spans_width(first_prefix);
    let mut line_width = prefix_width;
    let mut pending_space: Option<Style> = None;
    let width = width.max(prefix_width + 1);

    let mut new_line = |current: &mut Vec<Span<'static>>, line_width: &mut usize, prefix_width: &mut usize| {
        lines.push(Line::from(std::mem::replace(current, rest_prefix.to_vec())));
        *prefix_width = spans_width(rest_prefix);
        *line_width = *prefix_width;
    };

    for token in tokenize(spans) {
        match token {
            Token::Newline => {
                new_line(&mut current, &mut line_width, &mut prefix_width);
                pending_space = None;
            }
            Token::Space(style) => {
                if line_width > prefix_width {
                    pending_space = Some(style);
                }
            }
            Token::Word(pieces) => {
                let word_width: usize = pieces.iter().map(|(text, _)| text.width()).sum();
                let space_width = usize::from(pending_space.is_some());
                if line_width + space_width + word_width <= width {
                    if let Some(style) = pending_space.take() {
                        push_merged(&mut current, " ", style);
                        line_width += 1;
                    }
                    for (text, style) in &pieces {
                        push_merged(&mut current, text, *style);
                    }
                    line_width += word_width;
                    continue;
                }
                pending_space = None;
                if line_width > prefix_width {
                    new_line(&mut current, &mut line_width, &mut prefix_width);
                }
                // A word longer than the whole line gets split by characters.
                for (text, style) in &pieces {
                    for ch in text.chars() {
                        let ch_width = ch.width().unwrap_or(0);
                        if line_width + ch_width > width && line_width > prefix_width {
                            new_line(&mut current, &mut line_width, &mut prefix_width);
                        }
                        push_merged(&mut current, ch.encode_utf8(&mut [0; 4]), *style);
                        line_width += ch_width;
                    }
                }
            }
        }
    }
    if line_width > prefix_width {
        lines.push(Line::from(current));
    }
    lines
}

/// Splits styled text into chunks of at most `width` columns, ignoring word boundaries.
fn hard_wrap(spans: &[Span<'static>], width: usize) -> Vec<Vec<Span<'static>>> {
    let mut chunks = vec![Vec::new()];
    let mut used = 0;
    for span in spans {
        for ch in span.content.chars() {
            let ch_width = ch.width().unwrap_or(0);
            if used + ch_width > width && used > 0 {
                chunks.push(Vec::new());
                used = 0;
            }
            let chunk = chunks.last_mut().expect("chunks is never empty");
            push_merged(chunk, ch.encode_utf8(&mut [0; 4]), span.style);
            used += ch_width;
        }
    }
    chunks
}

fn syntax_set() -> &'static SyntaxSet {
    static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAXES.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme_set() -> &'static ThemeSet {
    static THEMES: OnceLock<ThemeSet> = OnceLock::new();
    THEMES.get_or_init(ThemeSet::load_defaults)
}

fn find_syntax<'a>(syntaxes: &'a SyntaxSet, lang: &str) -> &'a SyntaxReference {
    let lang = match lang.to_ascii_lowercase().as_str() {
        "ts" | "typescript" | "tsx" | "jsx" | "javascript" => "js".to_string(),
        "shell" | "console" | "zsh" => "bash".to_string(),
        other => other.to_string(),
    };
    syntaxes
        .find_syntax_by_token(&lang)
        .unwrap_or_else(|| syntaxes.find_syntax_plain_text())
}

/// Syntax-highlights code, returning one list of spans per source line.
fn highlight(code: &str, lang: &str) -> Vec<Vec<Span<'static>>> {
    let syntaxes = syntax_set();
    let theme = &theme_set().themes[SYNTAX_THEME];
    let mut highlighter = HighlightLines::new(find_syntax(syntaxes, lang), theme);
    let code = code.replace('\t', "    ");
    LinesWithEndings::from(&code)
        .map(|line| match highlighter.highlight_line(line, syntaxes) {
            Ok(ranges) => ranges
                .into_iter()
                .map(|(style, text)| {
                    let fg = style.foreground;
                    let text = text.trim_end_matches(['\n', '\r']).to_string();
                    Span::styled(text, Style::new().fg(Color::Rgb(fg.r, fg.g, fg.b)))
                })
                .filter(|span| !span.content.is_empty())
                .collect(),
            Err(_) => vec![Span::raw(line.trim_end_matches(['\n', '\r']).to_string())],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(markdown: &str, width: usize) -> Vec<String> {
        render(markdown, width)
            .lines
            .iter()
            .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
            .collect()
    }

    #[test]
    fn long_paragraph_fits_in_the_given_width_without_losing_words() {
        let text = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore.";

        let sut = render(text, 30);

        assert!(sut.lines.len() > 1);
        assert!(sut.lines.iter().all(|line| line.width() <= 30));
        let joined: Vec<String> = sut
            .lines
            .iter()
            .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        assert_eq!(joined.join(" "), text);
    }

    #[test]
    fn word_longer_than_the_line_is_split() {
        let lines = plain(&"x".repeat(50), 20);

        assert_eq!(lines, vec!["x".repeat(20), "x".repeat(20), "x".repeat(10)]);
    }

    #[test]
    fn punctuation_stays_attached_to_styled_word() {
        let lines = plain("aaaa bbbb cccc dddd **eeee**, ffff", 24);

        assert_eq!(lines, vec!["aaaa bbbb cccc dddd", "eeee, ffff"]);
    }

    #[test]
    fn top_level_headings_are_underlined() {
        let lines = plain("# Title\n\n## Section\n\ntext", 40);

        assert_eq!(lines, vec!["Title", "━━━━━", "", "Section", "───────", "", "text"]);
    }

    #[test]
    fn heading_positions_are_reported() {
        let sut = render("# One\n\ntext\n\n### Two\n\nmore", 40);

        assert_eq!(sut.headings, vec![0, 5]);
    }

    #[test]
    fn bullet_list_items_wrap_under_their_text() {
        let lines = plain("- first item with quite long text\n- second", 20);

        assert_eq!(lines, vec!["• first item with", "  quite long text", "• second"]);
    }

    #[test]
    fn ordered_list_continues_numbering_from_start_value() {
        let lines = plain("3. three\n4. four\n5. five", 40);

        assert_eq!(lines, vec!["3. three", "4. four", "5. five"]);
    }

    #[test]
    fn nested_lists_are_indented() {
        let lines = plain("- parent\n  - child\n- next", 40);

        assert_eq!(lines, vec!["• parent", "  ◦ child", "• next"]);
    }

    #[test]
    fn task_list_shows_checkboxes_instead_of_bullets() {
        let lines = plain("- [x] done\n- [ ] todo", 40);

        assert_eq!(lines, vec!["☑ done", "☐ todo"]);
    }

    #[test]
    fn blockquote_paragraphs_keep_the_bar_between_them() {
        let lines = plain("> one\n>\n> two\n\nafter", 40);

        assert_eq!(lines, vec!["│ one", "│", "│ two", "", "after"]);
    }

    #[test]
    fn link_shows_its_address_after_the_text() {
        let lines = plain("see [docs](https://example.com) and <https://x.dev>", 80);

        assert_eq!(lines, vec!["see docs (https://example.com) and https://x.dev"]);
    }

    #[test]
    fn code_block_keeps_line_breaks_and_indentation() {
        let lines = plain("```rust\nfn main() {\n    run();\n}\n```", 30);

        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0].trim(), "rust");
        assert_eq!(lines[1].trim_end(), "  fn main() {");
        assert_eq!(lines[2].trim_end(), "      run();");
        assert_eq!(lines[3].trim_end(), "  }");
        assert!(lines.iter().all(|line| line.chars().count() == 30));
    }

    #[test]
    fn table_is_drawn_with_borders_and_aligned_columns() {
        let lines = plain("| Name | Qty |\n|------|----:|\n| apple | 3 |\n| kiwi | 12 |", 40);

        assert_eq!(
            lines,
            vec![
                "┌───────┬─────┐",
                "│ Name  │ Qty │",
                "├───────┼─────┤",
                "│ apple │   3 │",
                "│ kiwi  │  12 │",
                "└───────┴─────┘",
            ]
        );
    }

    #[test]
    fn table_wider_than_screen_wraps_cell_text() {
        let lines = plain("| a | b |\n|---|---|\n| one two three four | x |", 20);

        assert!(lines.iter().all(|line| line.chars().count() <= 20));
        assert!(lines.len() > 5);
    }

    #[test]
    fn front_matter_is_hidden() {
        let lines = plain("---\ntitle: x\n---\n\nbody", 40);

        assert_eq!(lines, vec!["body"]);
    }
}
