mod cli;
mod render;
mod scan;
mod style;
mod tree;

use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;
use globset::{Glob, GlobSet, GlobSetBuilder};

use cli::Args;
use render::RenderOptions;
use scan::ScanOptions;
use style::Palette;

fn main() -> ExitCode {
    let args = Args::parse();
    let (patterns, excludes) = match (build_globs(&args.patterns), build_globs(&args.excludes)) {
        (Ok(patterns), Ok(excludes)) => (patterns, excludes),
        (Err(err), _) | (_, Err(err)) => {
            eprintln!("btree: invalid glob: {err}");
            return ExitCode::from(2);
        }
    };

    let mut out = BufWriter::new(io::stdout().lock());
    let result = args
        .paths
        .iter()
        .enumerate()
        .try_fold(true, |all_ok, (index, path)| {
            if index > 0 {
                writeln!(out)?;
            }
            let ok = print_tree(path, &args, &patterns, excludes.clone(), &mut out)?;
            Ok::<_, io::Error>(all_ok && ok)
        });

    match result.and_then(|all_ok| out.flush().map(|()| all_ok)) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        // `btree | head` closes the pipe early; that is not an error.
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("btree: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Prints one tree. Returns `Ok(false)` when the path or some of its entries could not be read.
fn print_tree(
    path: &Path,
    args: &Args,
    patterns: &GlobSet,
    excludes: GlobSet,
    out: &mut impl Write,
) -> io::Result<bool> {
    if let Err(err) = fs::symlink_metadata(path) {
        eprintln!("btree: {}: {err}", path.display());
        return Ok(false);
    }

    // One extra level is read so collapsed directories can show how many entries they hold.
    // Sizes and patterns need to see everything below the limit.
    let needs_full_walk = args.size || !args.patterns.is_empty();
    let scan_options = ScanOptions {
        show_hidden: args.all,
        respect_ignore_files: !args.no_ignore,
        max_depth: args
            .level
            .filter(|_| !needs_full_walk)
            .map(|level| level as usize + 1),
        exclude: excludes,
    };
    let (root, errors) = scan::scan(path, &scan_options);
    for err in &errors {
        eprintln!("btree: {err}");
    }
    let Some(mut root) = root else {
        return Ok(false);
    };

    if !args.patterns.is_empty() {
        tree::keep_files_matching(&mut root, &|name| patterns.is_match(name));
    }
    tree::compute_sizes(&mut root);
    if args.dirs_only {
        tree::remove_files(&mut root);
    }
    tree::sort(&mut root, args.sort);
    if let Some(level) = args.level {
        tree::limit_depth(&mut root, level as usize);
    }
    let stats = if root.is_dir() {
        tree::summarize(&root.children)
    } else {
        tree::summarize(std::slice::from_ref(&root))
    };
    if !args.no_collapse {
        tree::collapse_chains(&mut root);
    }

    let render_options = RenderOptions {
        show_sizes: args.size,
        max_entries: (args.max_entries > 0).then_some(args.max_entries),
        ascii: args.ascii,
    };
    let palette = Palette::new(args.color.is_enabled());
    write!(out, "{}", render::render(&root, &render_options, palette))?;
    if !args.no_summary {
        writeln!(out)?;
        writeln!(out, "{}", palette.dim(&render::summary(&stats, args.size)))?;
    }
    Ok(errors.is_empty() && stats.unreadable == 0)
}

fn build_globs(globs: &[String]) -> Result<GlobSet, globset::Error> {
    let mut builder = GlobSetBuilder::new();
    for glob in globs {
        builder.add(Glob::new(glob)?);
    }
    builder.build()
}
