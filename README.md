# terminal-programs

A collection of handy terminal programs. Each program lives in its own directory as a standalone Cargo project.

| Program | Description |
|---------|-------------|
| [btree](btree/) | Better `tree` — readable directory tree in the terminal |
| [tmd](tmd/) | Terminal Markdown viewer (ratatui + syntect) |

## Install system-wide

Installs the binaries into `~/.cargo/bin` (make sure it is on your `PATH`):

```sh
# all programs
for p in btree tmd; do cargo install --path "$p"; done

# a single program
cargo install --path btree
```

Uninstall with `cargo uninstall <program>`.

## Build

```sh
cd <program> && cargo build --release
```
