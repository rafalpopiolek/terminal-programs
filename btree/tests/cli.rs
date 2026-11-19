use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

#[test]
fn directories_are_listed_before_files_and_both_in_name_order() {
    let workspace = project(&[
        "src/main.rs",
        "src/lib.rs",
        "README.md",
        "Cargo.toml",
        "docs/guide.md",
    ]);

    let output = btree(&workspace, &[]);

    assert_eq!(
        output,
        "\
project/
├─ docs/
│  ╰─ guide.md
├─ src/
│  ├─ lib.rs
│  ╰─ main.rs
├─ Cargo.toml
╰─ README.md

2 directories, 5 files
"
    );
}

#[test]
fn numbered_files_are_ordered_by_their_number() {
    let workspace = project(&["part10.txt", "part2.txt", "part1.txt"]);

    let output = btree(&workspace, &["--no-summary"]);

    assert_eq!(
        output,
        "project/\n├─ part1.txt\n├─ part2.txt\n╰─ part10.txt\n"
    );
}

#[test]
fn files_ignored_by_git_are_not_shown() {
    let workspace = project(&["build/out.bin", "main.rs"]);
    write(&workspace, "project/.gitignore", "build/\n");

    let output = btree(&workspace, &["--no-summary"]);

    assert_eq!(output, "project/\n╰─ main.rs\n");
}

#[test]
fn files_ignored_by_git_are_shown_on_request() {
    let workspace = project(&["build/out.bin", "main.rs"]);
    write(&workspace, "project/.gitignore", "build/\n");

    let output = btree(&workspace, &["--no-summary", "--no-ignore"]);

    assert_eq!(output, "project/\n├─ build/\n│  ╰─ out.bin\n╰─ main.rs\n");
}

#[test]
fn dotfiles_are_shown_only_on_request() {
    let workspace = project(&[".env", "main.rs"]);

    let default_output = btree(&workspace, &["--no-summary"]);
    let all_output = btree(&workspace, &["--no-summary", "--all"]);

    assert_eq!(default_output, "project/\n╰─ main.rs\n");
    assert_eq!(all_output, "project/\n├─ .env\n╰─ main.rs\n");
}

#[test]
fn git_internals_stay_hidden_even_when_showing_everything() {
    let workspace = project(&[".git/HEAD", "main.rs"]);

    let output = btree(&workspace, &["--no-summary", "--all"]);

    assert_eq!(output, "project/\n╰─ main.rs\n");
}

#[test]
fn excluded_names_are_skipped_together_with_their_contents() {
    let workspace = project(&["node_modules/left-pad/index.js", "app.js"]);

    let output = btree(&workspace, &["--no-summary", "-I", "node_modules"]);

    assert_eq!(output, "project/\n╰─ app.js\n");
}

#[test]
fn directories_holding_only_one_subdirectory_are_shown_as_one_path() {
    let workspace = project(&["src/main/java/App.java", "README.md"]);

    let output = btree(&workspace, &["--no-summary"]);

    assert_eq!(
        output,
        "project/\n├─ src/main/java/\n│  ╰─ App.java\n╰─ README.md\n"
    );
}

#[test]
fn single_subdirectory_chains_can_be_shown_level_by_level() {
    let workspace = project(&["src/main/App.java"]);

    let output = btree(&workspace, &["--no-summary", "--no-collapse"]);

    assert_eq!(
        output,
        "project/\n╰─ src/\n   ╰─ main/\n      ╰─ App.java\n"
    );
}

#[test]
fn directories_beyond_the_level_limit_tell_how_much_they_hold() {
    let workspace = project(&["src/a.rs", "src/b.rs", "src/nested/c.rs", "top.txt"]);

    let output = btree(&workspace, &["-L", "1"]);

    assert_eq!(
        output,
        "project/\n├─ src/ … 3 entries\n╰─ top.txt\n\n1 directory, 1 file\n"
    );
}

#[test]
fn pattern_keeps_only_matching_files_and_the_directories_leading_to_them() {
    let workspace = project(&[
        "src/main.rs",
        "src/notes.txt",
        "docs/guide.md",
        "Cargo.toml",
    ]);

    let output = btree(&workspace, &["-P", "*.rs"]);

    assert_eq!(
        output,
        "project/\n╰─ src/\n   ╰─ main.rs\n\n1 directory, 1 file\n"
    );
}

#[test]
fn directories_only_mode_lists_no_files() {
    let workspace = project(&["src/main.rs", "docs/guide.md", "Cargo.toml"]);

    let output = btree(&workspace, &["-d"]);

    assert_eq!(
        output,
        "project/\n├─ docs/\n╰─ src/\n\n2 directories, 0 files\n"
    );
}

#[test]
fn long_directories_are_shortened_with_a_note_about_the_rest() {
    let workspace = project(&["a.txt", "b.txt", "c.txt", "d.txt", "e/"]);

    let output = btree(&workspace, &["-n", "2"]);

    assert_eq!(
        output,
        "project/\n├─ e/\n├─ a.txt\n╰─ … 3 more files\n\n1 directory, 4 files\n"
    );
}

#[test]
fn sizes_are_right_aligned_in_one_column_and_directories_show_their_total() {
    let workspace = project(&[]);
    write(&workspace, "project/big.bin", &"x".repeat(2048));
    write(&workspace, "project/small.txt", &"x".repeat(10));
    write(&workspace, "project/src/code.rs", &"x".repeat(100));

    let output = btree(&workspace, &["-s"]);

    assert_eq!(
        output,
        "\
project/       2.1 KiB
├─ src/          100 B
│  ╰─ code.rs    100 B
├─ big.bin     2.0 KiB
╰─ small.txt      10 B

1 directory, 3 files, 2.1 KiB total
"
    );
}

#[test]
fn larger_entries_come_first_when_sorting_by_size() {
    let workspace = project(&[]);
    write(&workspace, "project/small.txt", "x");
    write(&workspace, "project/large.txt", &"x".repeat(500));

    let output = btree(&workspace, &["--no-summary", "--sort", "size"]);

    assert_eq!(output, "project/\n├─ large.txt\n╰─ small.txt\n");
}

#[test]
fn ascii_mode_draws_the_tree_without_box_drawing_characters() {
    let workspace = project(&["src/main.rs", "README.md"]);

    let output = btree(&workspace, &["--no-summary", "--ascii"]);

    assert_eq!(output, "project/\n|- src/\n|  `- main.rs\n`- README.md\n");
}

#[cfg(unix)]
#[test]
fn symbolic_links_show_where_they_point() {
    let workspace = project(&["main.rs"]);
    std::os::unix::fs::symlink("main.rs", workspace.path().join("project/link")).unwrap();

    let output = btree(&workspace, &["--no-summary"]);

    assert_eq!(output, "project/\n├─ link → main.rs\n╰─ main.rs\n");
}

#[test]
fn a_file_given_instead_of_a_directory_is_listed_on_its_own() {
    let workspace = project(&["notes.txt"]);

    let output = btree(&workspace, &[]);

    assert_eq!(output, "project/\n╰─ notes.txt\n\n0 directories, 1 file\n");
    let file_output = run(&workspace, "project/notes.txt", &[]);
    assert_eq!(
        String::from_utf8(file_output.stdout).unwrap(),
        "project/notes.txt\n\n0 directories, 1 file\n"
    );
}

#[cfg(unix)]
#[test]
fn directories_that_cannot_be_read_are_marked_in_place() {
    use std::os::unix::fs::PermissionsExt;
    let workspace = project(&["secret/plans.txt", "main.rs"]);
    let secret = workspace.path().join("project/secret");
    fs::set_permissions(&secret, fs::Permissions::from_mode(0o000)).unwrap();

    let output = run(&workspace, "project", &[]);

    fs::set_permissions(&secret, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "project/\n├─ secret/ (cannot read)\n╰─ main.rs\n\n1 directory, 1 file, 1 unreadable\n"
    );
}

#[test]
fn a_missing_path_is_reported_as_a_failure() {
    let workspace = project(&[]);

    let output = run(&workspace, "missing", &[]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("btree: missing"));
}

/// Creates `<tmp>/project` with the given entries; entries ending with `/` are directories.
fn project(entries: &[&str]) -> TempDir {
    let workspace = tempfile::tempdir().unwrap();
    fs::create_dir(workspace.path().join("project")).unwrap();
    for entry in entries {
        if entry.ends_with('/') {
            fs::create_dir_all(workspace.path().join("project").join(entry)).unwrap();
        } else {
            write(&workspace, &format!("project/{entry}"), "x");
        }
    }
    workspace
}

fn write(workspace: &TempDir, relative_path: &str, contents: &str) {
    let path = workspace.path().join(relative_path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// Runs btree on `<tmp>/project` and returns its successful output.
fn btree(workspace: &TempDir, args: &[&str]) -> String {
    let output = run(workspace, "project", args);
    assert!(
        output.status.success(),
        "btree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn run(workspace: &TempDir, target: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_btree"))
        .current_dir(workspace.path())
        .arg(Path::new(target))
        .args(["--color", "never"])
        .args(args)
        .output()
        .unwrap()
}
