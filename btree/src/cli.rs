use std::io::IsTerminal;
use std::path::PathBuf;

use clap::{Parser, ValueEnum};

/// A more readable `tree`: directories first, colors, .gitignore awareness,
/// collapsed single-directory chains and an aligned size column.
#[derive(Debug, Parser)]
#[command(name = "btree", version, about)]
pub struct Args {
    /// Directories to display
    #[arg(default_value = ".")]
    pub paths: Vec<PathBuf>,

    /// Show hidden files (dotfiles); `.git` is always skipped
    #[arg(short, long)]
    pub all: bool,

    /// Show files ignored by .gitignore, .ignore and git exclude files
    #[arg(long)]
    pub no_ignore: bool,

    /// List directories only
    #[arg(short, long)]
    pub dirs_only: bool,

    /// Descend at most LEVEL directories deep
    #[arg(short = 'L', long, value_parser = clap::value_parser!(u32).range(1..))]
    pub level: Option<u32>,

    /// Show only files matching the glob (repeatable), e.g. -P '*.rs'
    #[arg(short = 'P', long = "pattern", value_name = "GLOB")]
    pub patterns: Vec<String>,

    /// Skip files and directories matching the glob (repeatable), e.g. -I node_modules
    #[arg(short = 'I', long = "exclude", value_name = "GLOB")]
    pub excludes: Vec<String>,

    /// Show file sizes and total directory sizes in an aligned column
    #[arg(short, long)]
    pub size: bool,

    /// Order of entries inside a directory (directories always come first)
    #[arg(long, value_enum, default_value_t = SortBy::Name)]
    pub sort: SortBy,

    /// Show at most N entries per directory, summarizing the rest (0 = no limit)
    #[arg(short = 'n', long, value_name = "N", default_value_t = 50)]
    pub max_entries: usize,

    /// Do not merge chains of single-child directories into one line (a/b/c/)
    #[arg(long)]
    pub no_collapse: bool,

    /// Draw the tree with ASCII characters only
    #[arg(long)]
    pub ascii: bool,

    /// When to use colors
    #[arg(long, value_enum, default_value_t = ColorMode::Auto)]
    pub color: ColorMode,

    /// Do not print the summary line
    #[arg(long)]
    pub no_summary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SortBy {
    /// Natural, case-insensitive name order (file2 before file10)
    Name,
    /// Largest first
    Size,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

impl ColorMode {
    pub fn is_enabled(self) -> bool {
        match self {
            ColorMode::Always => true,
            ColorMode::Never => false,
            ColorMode::Auto => {
                let no_color = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
                std::io::stdout().is_terminal() && !no_color
            }
        }
    }
}
