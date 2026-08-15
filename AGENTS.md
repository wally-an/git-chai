# git-chai

Automates staging and committing with intelligent grouping. Usage and
user-facing behavior: README.md. This file covers what is not obvious from
the code.

## Commands

```bash
cargo build                          # build
cargo test                           # unit + integration tests
cargo clippy --all-targets -- -D warnings
cargo fmt --check
nix build                            # flake package; runs cargo test in the sandbox
nix run .# -- --version              # runs the flake-built binary
nix flake check
```

## Architecture

main.rs           CLI (clap) + orchestration loop; headless poller
git/mod.rs        the only module that shells out to git
git/status.rs     porcelain -z scan -> Vec<GitChange>
git/grouping.rs   Vec<GitChange> -> Vec<ChangeGroup>, deterministic
git/commit.rs     isolated commit per group, push, messages
types.rs, error.rs

Data flow: scan -> group -> commit -> optional push.

## Invariants (do not "fix" these)

- Commits contain exactly the paths they name: `commit_paths` stages into a
  throwaway index seeded from HEAD and syncs the real index with
  `git reset -- <paths>`. A plain `git commit -m` sweeps the user's
  pre-staged changes.
- Unmerged paths are never committed.
- Root files, renames, and copies always commit individually; a directory
  commits as one unit only when every file in it (tracked and untracked)
  changed with the same type.
- Empty `RUST_LOG` means unset; the tool must still log in sandboxes.
- Version lives only in `Cargo.toml`; flake and `--version` derive it.
- Filenames use `:(literal)` pathspecs; glob characters are filenames.

## Decisions

- git CLI over libgit2/gix: hooks, filters, auth, and pathspec magic come
  free; process spawns are not a bottleneck at the 5-second poll. The
  functional suite is the acceptance test for any migration.
- `installer.sh` stays at the repo root: its raw URL is the documented
  install interface.

## Style and workflow

- rustfmt and clippy `-D warnings` are the source of truth; thiserror inside
  `git::`, anyhow at the main boundary; `log` crate, info = outcomes,
  debug = detail; no unsafe, no magic strings.
- One commit per file: `mod: <path>` | `add: <path>` | `del: <path>`.
- CI runs fmt, clippy, `cargo test`, and `tests/functional.sh` on every
  push; run them locally before pushing.
- Guarantee tests are regressions: fix the code, never loosen the test.
- Releases: bump the version in `Cargo.toml` only, then `cargo test`,
  `nix build`, and `tests/functional.sh` against the flake-built binary.
