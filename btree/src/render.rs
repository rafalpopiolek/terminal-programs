//! Turns a [`Node`] tree into terminal text.

use unicode_width::UnicodeWidthStr;

use crate::style::Palette;
use crate::tree::{Kind, Node, Stats};

pub struct RenderOptions {
    pub show_sizes: bool,
    /// Entries shown per directory before the rest is summarized.
    pub max_entries: Option<usize>,
    pub ascii: bool,
}

struct Symbols {
    tee: &'static str,
    corner: &'static str,
    pipe: &'static str,
    space: &'static str,
    ellipsis: &'static str,
    arrow: &'static str,
}

const UNICODE: Symbols = Symbols {
    tee: "├─ ",
    corner: "╰─ ",
    pipe: "│  ",
    space: "   ",
    ellipsis: "…",
    arrow: " → ",
};

const ASCII: Symbols = Symbols {
    tee: "|- ",
    corner: "`- ",
    pipe: "|  ",
    space: "   ",
    ellipsis: "...",
    arrow: " -> ",
};

/// Text kept twice: plain (to measure its width) and colored (to print).
#[derive(Default)]
struct Label {
    plain: String,
    styled: String,
}

impl Label {
    fn push(&mut self, plain: &str, styled: String) {
        self.plain.push_str(plain);
        self.styled.push_str(&styled);
    }
}

struct Row {
    guide: String,
    label: Label,
    size: u64,
}

impl Row {
    fn width(&self) -> usize {
        self.guide.width() + self.label.plain.width()
    }
}

pub fn render(root: &Node, options: &RenderOptions, palette: Palette) -> String {
    let symbols = if options.ascii { &ASCII } else { &UNICODE };
    let mut rows = vec![Row {
        guide: String::new(),
        label: node_label(root, symbols, palette),
        size: root.size,
    }];
    collect_rows(root, "", symbols, options, palette, &mut rows);
    format_rows(&rows, options.show_sizes, palette)
}

pub fn summary(stats: &Stats, show_size: bool) -> String {
    let mut text = format!(
        "{} {}, {} {}",
        stats.dirs,
        plural(stats.dirs, "directory", "directories"),
        stats.files,
        plural(stats.files, "file", "files"),
    );
    if stats.unreadable > 0 {
        text.push_str(&format!(", {} unreadable", stats.unreadable));
    }
    if show_size {
        text.push_str(&format!(", {} total", human_size(stats.bytes)));
    }
    text
}

fn collect_rows(
    node: &Node,
    prefix: &str,
    symbols: &Symbols,
    options: &RenderOptions,
    palette: Palette,
    rows: &mut Vec<Row>,
) {
    let shown_count = match options.max_entries {
        Some(max) => node.children.len().min(max),
        None => node.children.len(),
    };
    let (shown, hidden) = node.children.split_at(shown_count);

    for (index, child) in shown.iter().enumerate() {
        let is_last = index + 1 == shown.len() && hidden.is_empty();
        let connector = if is_last { symbols.corner } else { symbols.tee };
        rows.push(Row {
            guide: format!("{prefix}{connector}"),
            label: node_label(child, symbols, palette),
            size: child.size,
        });
        if !child.children.is_empty() {
            let indent = if is_last { symbols.space } else { symbols.pipe };
            collect_rows(
                child,
                &format!("{prefix}{indent}"),
                symbols,
                options,
                palette,
                rows,
            );
        }
    }

    if !hidden.is_empty() {
        let text = format!("{} {}", symbols.ellipsis, describe_hidden(hidden));
        let mut label = Label::default();
        label.push(&text, palette.dim(&text));
        rows.push(Row {
            guide: format!("{prefix}{}", symbols.corner),
            label,
            size: hidden.iter().map(|n| n.size).sum(),
        });
    }
}

fn describe_hidden(hidden: &[Node]) -> String {
    let dirs = hidden.iter().filter(|n| n.is_dir()).count();
    let files = hidden.len() - dirs;
    match (dirs, files) {
        (0, files) => format!("{files} more {}", plural(files, "file", "files")),
        (dirs, 0) => format!("{dirs} more {}", plural(dirs, "directory", "directories")),
        (dirs, files) => format!(
            "{} more ({dirs} {}, {files} {})",
            hidden.len(),
            plural(dirs, "directory", "directories"),
            plural(files, "file", "files"),
        ),
    }
}

fn node_label(node: &Node, symbols: &Symbols, palette: Palette) -> Label {
    let mut label = Label::default();
    match &node.kind {
        Kind::Dir => {
            let name = if node.name.ends_with('/') {
                node.name.clone()
            } else {
                format!("{}/", node.name)
            };
            label.push(&name, palette.dir(&name));
            if node.unreadable {
                let note = " (cannot read)";
                label.push(note, palette.broken(note));
            }
            if node.unexpanded > 0 {
                let hint = format!(
                    " {} {} {}",
                    symbols.ellipsis,
                    node.unexpanded,
                    plural(node.unexpanded, "entry", "entries")
                );
                label.push(&hint, palette.dim(&hint));
            }
        }
        Kind::File { executable } => {
            let styled = if *executable {
                palette.executable(&node.name)
            } else if node.is_hidden() {
                palette.dim(&node.name)
            } else {
                node.name.clone()
            };
            label.push(&node.name, styled);
        }
        Kind::Symlink { target, broken } => {
            label.push(&node.name, palette.symlink(&node.name));
            label.push(symbols.arrow, palette.dim(symbols.arrow));
            let styled_target = if *broken {
                palette.broken(target)
            } else {
                palette.dim(target)
            };
            label.push(target, styled_target);
        }
    }
    label
}

fn format_rows(rows: &[Row], show_sizes: bool, palette: Palette) -> String {
    let name_width = rows.iter().map(Row::width).max().unwrap_or(0);
    let sizes: Vec<String> = rows.iter().map(|row| human_size(row.size)).collect();
    let size_width = sizes.iter().map(String::len).max().unwrap_or(0);

    let mut out = String::new();
    for (row, size) in rows.iter().zip(&sizes) {
        out.push_str(&palette.dim(&row.guide));
        out.push_str(&row.label.styled);
        if show_sizes {
            let padding = name_width - row.width() + 2;
            out.push_str(&format!("{}{size:>size_width$}", " ".repeat(padding)));
        }
        out.push('\n');
    }
    out
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

fn plural(count: usize, one: &'static str, many: &'static str) -> &'static str {
    if count == 1 { one } else { many }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_shown_in_the_largest_fitting_unit() {
        let cases = [
            ("zero", 0, "0 B"),
            ("just below a kibibyte", 1023, "1023 B"),
            ("exactly a kibibyte", 1024, "1.0 KiB"),
            ("one and a half kibibytes", 1536, "1.5 KiB"),
            ("five mebibytes", 5 * 1024 * 1024, "5.0 MiB"),
            ("three gibibytes", 3 * 1024 * 1024 * 1024, "3.0 GiB"),
        ];

        for (case, bytes, expected) in cases {
            assert_eq!(human_size(bytes), expected, "case: {case}");
        }
    }

    #[test]
    fn summary_uses_singular_for_one_item() {
        let stats = Stats {
            dirs: 1,
            files: 1,
            unreadable: 0,
            bytes: 2048,
        };

        let text = summary(&stats, true);

        assert_eq!(text, "1 directory, 1 file, 2.0 KiB total");
    }
}
