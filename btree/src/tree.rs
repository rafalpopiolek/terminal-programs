//! In-memory directory tree and the pure transformations applied before rendering.

use std::cmp::Ordering;

use crate::cli::SortBy;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Dir,
    File { executable: bool },
    Symlink { target: String, broken: bool },
}

#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub kind: Kind,
    /// File size, or the total size of everything below a directory.
    pub size: u64,
    pub children: Vec<Node>,
    /// Direct children hidden because of the depth limit.
    pub unexpanded: usize,
    /// The directory exists but its contents could not be listed (e.g. no permission).
    pub unreadable: bool,
}

impl Node {
    pub fn is_dir(&self) -> bool {
        self.kind == Kind::Dir
    }

    pub fn is_hidden(&self) -> bool {
        self.name.starts_with('.')
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub dirs: usize,
    pub files: usize,
    pub unreadable: usize,
    pub bytes: u64,
}

/// Counts every directory and file in `nodes` (recursively) and sums their sizes.
pub fn summarize(nodes: &[Node]) -> Stats {
    let mut stats = Stats::default();
    for node in nodes {
        stats.bytes += node.size;
        if node.is_dir() {
            let inner = summarize(&node.children);
            stats.dirs += 1 + inner.dirs;
            stats.files += inner.files;
            stats.unreadable += usize::from(node.unreadable) + inner.unreadable;
        } else {
            stats.files += 1;
        }
    }
    stats
}

/// Keeps only files accepted by `is_match` and drops directories left without any.
pub fn keep_files_matching(node: &mut Node, is_match: &dyn Fn(&str) -> bool) {
    node.children.retain_mut(|child| {
        if child.is_dir() {
            keep_files_matching(child, is_match);
            !child.children.is_empty()
        } else {
            is_match(&child.name)
        }
    });
}

pub fn remove_files(node: &mut Node) {
    node.children.retain(Node::is_dir);
    node.children.iter_mut().for_each(remove_files);
}

/// Sets every directory's size to the total size of its contents and returns the node's size.
pub fn compute_sizes(node: &mut Node) -> u64 {
    if node.is_dir() {
        node.size = node.children.iter_mut().map(compute_sizes).sum();
    }
    node.size
}

/// Directories first, then entries ordered by `sort_by`.
pub fn sort(node: &mut Node, sort_by: SortBy) {
    node.children.sort_by(|a, b| {
        b.is_dir().cmp(&a.is_dir()).then_with(|| match sort_by {
            SortBy::Name => natural_cmp(&a.name, &b.name),
            SortBy::Size => b
                .size
                .cmp(&a.size)
                .then_with(|| natural_cmp(&a.name, &b.name)),
        })
    });
    node.children
        .iter_mut()
        .for_each(|child| sort(child, sort_by));
}

/// Hides everything deeper than `levels` below `node`, remembering how many entries were hidden.
pub fn limit_depth(node: &mut Node, levels: usize) {
    if levels == 0 {
        node.unexpanded = node.children.len();
        node.children.clear();
        return;
    }
    node.children
        .iter_mut()
        .for_each(|child| limit_depth(child, levels - 1));
}

/// Merges directories that contain nothing but a single subdirectory: `src/` + `main/` -> `src/main/`.
pub fn collapse_chains(node: &mut Node) {
    for child in &mut node.children {
        while child.is_dir()
            && child.unexpanded == 0
            && child.children.len() == 1
            && child.children[0].is_dir()
        {
            let only = child.children.remove(0);
            child.name = format!("{}/{}", child.name.trim_end_matches('/'), only.name);
            child.children = only.children;
            child.unexpanded = only.unexpanded;
            child.unreadable = only.unreadable;
        }
        collapse_chains(child);
    }
}

/// Case-insensitive comparison that orders digit runs by numeric value (`file2` < `file10`).
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut left = a.chars().peekable();
    let mut right = b.chars().peekable();
    loop {
        let ordering = match (left.peek(), right.peek()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let x = take_digits(&mut left);
                let y = take_digits(&mut right);
                let (x, y) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                x.len().cmp(&y.len()).then_with(|| x.cmp(y))
            }
            (Some(x), Some(y)) => {
                let ordering = x.to_lowercase().cmp(y.to_lowercase());
                left.next();
                right.next();
                ordering
            }
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
}

fn take_digits(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut digits = String::new();
    while let Some(c) = chars.next_if(char::is_ascii_digit) {
        digits.push(c);
    }
    digits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_ordered_like_a_human_would_order_them() {
        let mut names = vec![
            "file10.txt",
            "file2.txt",
            "File1.txt",
            "beta",
            "Alpha",
            "file003.txt",
        ];

        names.sort_by(|a, b| natural_cmp(a, b));

        assert_eq!(
            names,
            vec![
                "Alpha",
                "beta",
                "File1.txt",
                "file2.txt",
                "file003.txt",
                "file10.txt"
            ]
        );
    }

    #[test]
    fn names_differing_only_by_case_still_have_a_stable_order() {
        let mut names = vec!["readme", "README"];

        names.sort_by(|a, b| natural_cmp(a, b));

        assert_eq!(names, vec!["README", "readme"]);
    }

    #[test]
    fn a_directory_next_to_a_file_is_not_merged_into_a_chain() {
        let mut sut = dir(
            "root",
            vec![dir("src", vec![dir("main", vec![]), file("lib.rs")])],
        );

        collapse_chains(&mut sut);

        assert_eq!(sut.children[0].name, "src");
        assert_eq!(sut.children[0].children.len(), 2);
    }

    fn dir(name: &str, children: Vec<Node>) -> Node {
        Node {
            name: name.into(),
            kind: Kind::Dir,
            size: 0,
            children,
            unexpanded: 0,
            unreadable: false,
        }
    }

    fn file(name: &str) -> Node {
        Node {
            name: name.into(),
            kind: Kind::File { executable: false },
            size: 0,
            children: vec![],
            unexpanded: 0,
            unreadable: false,
        }
    }
}
