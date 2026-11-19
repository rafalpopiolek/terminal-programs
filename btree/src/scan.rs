//! Reads the filesystem into a [`Node`] tree.

use std::fs;
use std::path::{Path, PathBuf};

use globset::GlobSet;
use ignore::{DirEntry, WalkBuilder};

use crate::tree::{Kind, Node};

pub struct ScanOptions {
    pub show_hidden: bool,
    pub respect_ignore_files: bool,
    /// Walk depth counted from the root (root = 0); `None` walks everything.
    pub max_depth: Option<usize>,
    pub exclude: GlobSet,
}

/// Walks `root` and returns its tree (`None` when the root itself cannot be read).
/// Directories that cannot be listed are marked as unreadable; any other problems are returned as errors.
pub fn scan(root: &Path, options: &ScanOptions) -> (Option<Node>, Vec<ignore::Error>) {
    let respect = options.respect_ignore_files;
    let exclude = options.exclude.clone();
    let walker = WalkBuilder::new(root)
        .hidden(!options.show_hidden)
        .ignore(respect)
        .git_ignore(respect)
        .git_global(respect)
        .git_exclude(respect)
        .parents(respect)
        .require_git(false)
        .follow_links(false)
        .max_depth(options.max_depth)
        .filter_entry(move |entry| {
            entry.depth() == 0
                || (entry.file_name() != ".git" && !exclude.is_match(entry.file_name()))
        })
        .build();

    // The walk is depth-first, so `open_dirs[d]` is always the directory being filled at depth `d`.
    let mut open_dirs: Vec<OpenDir> = Vec::new();
    let mut errors = Vec::new();
    for result in walker {
        let entry = match result {
            Ok(entry) => entry,
            Err(err) => {
                // A directory that cannot be listed fails right after it was yielded, so it is on top.
                match open_dirs.last_mut() {
                    Some(dir) if error_path(&err) == Some(dir.path.as_path()) => {
                        dir.node.unreadable = true;
                    }
                    _ => errors.push(err),
                }
                continue;
            }
        };
        while open_dirs.len() > entry.depth() {
            close_last_dir(&mut open_dirs);
        }
        let node = node_from_entry(&entry);
        match open_dirs.last_mut() {
            Some(parent) if !node.is_dir() => parent.node.children.push(node),
            _ => open_dirs.push(OpenDir {
                path: entry.into_path(),
                node,
            }),
        }
    }
    while open_dirs.len() > 1 {
        close_last_dir(&mut open_dirs);
    }
    (open_dirs.pop().map(|dir| dir.node), errors)
}

struct OpenDir {
    path: PathBuf,
    node: Node,
}

fn close_last_dir(open_dirs: &mut Vec<OpenDir>) {
    let finished = open_dirs
        .pop()
        .expect("caller checks the stack is not empty");
    if let Some(parent) = open_dirs.last_mut() {
        parent.node.children.push(finished.node);
    }
}

fn error_path(err: &ignore::Error) -> Option<&Path> {
    match err {
        ignore::Error::WithPath { path, .. } => Some(path),
        ignore::Error::WithDepth { err, .. } | ignore::Error::WithLineNumber { err, .. } => {
            error_path(err)
        }
        _ => None,
    }
}

fn node_from_entry(entry: &DirEntry) -> Node {
    let path = entry.path();
    let name = if entry.depth() == 0 {
        root_name(path)
    } else {
        entry.file_name().to_string_lossy().into_owned()
    };
    let metadata = entry.metadata().ok();
    let file_type = entry.file_type();

    let kind = if file_type.is_some_and(|t| t.is_symlink()) {
        Kind::Symlink {
            target: fs::read_link(path)
                .map(|t| t.display().to_string())
                .unwrap_or_default(),
            broken: fs::metadata(path).is_err(),
        }
    } else if file_type.is_some_and(|t| t.is_dir()) {
        Kind::Dir
    } else {
        Kind::File {
            executable: metadata.as_ref().is_some_and(is_executable),
        }
    };
    let size = match kind {
        Kind::Dir => 0,
        _ => metadata.map_or(0, |m| m.len()),
    };

    Node {
        name,
        kind,
        size,
        children: Vec::new(),
        unexpanded: 0,
        unreadable: false,
    }
}

/// `.` or `..` say nothing about where you are, so show the real directory name instead.
fn root_name(path: &Path) -> String {
    if path.file_name().is_none()
        && let Some(name) = path
            .canonicalize()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_owned()))
    {
        return name.to_string_lossy().into_owned();
    }
    path.display().to_string()
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &fs::Metadata) -> bool {
    false
}
