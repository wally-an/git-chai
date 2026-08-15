# Chai

A git automation tool that automatically stages and commits changes with intelligent grouping.

## Requirements

- `git` - for version control operations
- `curl` and `tar` - for downloading and installation
- `rustup` - for building Rust binaries (will be installed automatically if missing)

## Installation

```bash
curl -sSL https://raw.githubusercontent.com/wally-an/git-chai/main/installer.sh | bash
```

Or build from source:

```bash
cargo build --release
# binary at target/release/git-chai
```

Nix users: `nix build` or `nix develop` for a dev shell.

## Usage

Run `git-chai` in your Git repository to automatically stage and commit changes:

```bash
git-chai [OPTIONS]
```

### Options

| Short | Long | Description |
|-------|------|-------------|
| `-r` | `--repo-path` | Path to git repository (default: current directory) |
| `-p` | `--push` | Push changes to remote after committing (default: false) |
| `-d` | `--dry-run` | Show what would be committed without actually committing |
| `-v` | `--verbose` | Enable verbose output |
| `-!` | `--headless` | Run continuously until interrupted (headless mode) |
| `-?` | `--version` | Show version information |

### How changes are grouped

Each scan of `git status` produces a set of commits:

- **Directory commit**: when *every* file in a directory (tracked and untracked)
  changed with the same change type, the whole directory is committed as one,
  e.g. `mod: src`.
- **Individual commit**: otherwise each file is committed separately with a
  precise message, e.g. `mod: src/main.rs`, `add: new-file.txt`,
  `del: removed.txt`.
- **Rename / copy**: `rename: src/a.rs -> src/b.rs` (also `copy:`).
- **Untracked directory**: committed as one `add: dirname`.
- **Root-level files** always commit individually.

Guarantees:

- Commits contain exactly the paths they name: anything you had staged before
  running git-chai is never swept into an unrelated commit.
- Unmerged (conflicted) paths are skipped with a warning, never committed.
- Failed commits are reported and reflected in the exit code (non-zero).

### Examples

#### Level 1: Basic Commit Operations
```bash
# Commit changes once (no push)
git-chai

# Dry-run to preview what would be committed
git-chai --dry-run
```

#### Level 2: Enhanced Commit Operations
```bash
# Commit with verbose output for debugging
git-chai --verbose

# Commit changes in specific repository
git-chai --repo-path /path/to/repo

# Preview commits with verbose details
git-chai --dry-run --verbose
```

#### Level 3: Commit + Push Operations
```bash
# Commit and push changes once
git-chai --push

# Commit and push with verbose output
git-chai --push --verbose

# Commit and push from specific repository
git-chai --repo-path /path/to/repo --push
```

#### Level 4: Complete Autonomy
```bash
# Continuous monitoring with auto-commit (no push)
git-chai --headless

# Fully autonomous: continuous commit + push
git-chai --headless --push

# Autonomous with detailed logging
git-chai --headless --push --verbose

# Development Workflow:
# Terminal 1: git-chai --headless --push
# Terminal 2: # Keep coding - changes auto-committed & pushed
```

Headless mode scans every 5 seconds and keeps running until interrupted
(Ctrl+C).

## Development

```bash
cargo test       # unit + integration tests (spawn real git repos)
cargo clippy --all-targets
cargo fmt --check
nix develop      # or nix-shell -p cargo rustc clippy rustfmt
```
