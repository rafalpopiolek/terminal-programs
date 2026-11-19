mod markdown;
mod viewer;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyEventKind};

use viewer::{Outcome, Viewer};

const USAGE: &str = "usage: tmd <file.md>\n\nPress ? inside the viewer to see key bindings.";

fn main() -> Result<()> {
    let path = parse_args()?;
    let source = read(&path)?;
    let title = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    let mut viewer = Viewer::new(title, source);

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut viewer, &path);
    ratatui::restore();
    result
}

fn parse_args() -> Result<PathBuf> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [flag] if flag == "-h" || flag == "--help" => {
            println!("{USAGE}");
            std::process::exit(0);
        }
        [path] => Ok(PathBuf::from(path)),
        _ => bail!(USAGE),
    }
}

fn read(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn run(terminal: &mut DefaultTerminal, viewer: &mut Viewer, path: &Path) -> Result<()> {
    loop {
        terminal.draw(|frame| viewer.draw(frame))?;
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match viewer.handle_key(key) {
                Outcome::Continue => {}
                Outcome::Quit => return Ok(()),
                Outcome::Reload => match read(path) {
                    Ok(source) => viewer.reload(source),
                    Err(error) => viewer.show_error(format!("{error:#}")),
                },
            },
            _ => {}
        }
    }
}
