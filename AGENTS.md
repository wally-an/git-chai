# git-chai

Automates staging and committing with intelligent grouping. Usage and user-facing behavior: see README.md. This file covers development workflow, architecture, and rules that are not obvious from the code.

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

`cargo` is the primary path; the nix commands exist because the sandbox test run catches environment-specific bugs (it caught one: empty `RUST_LOG` muting all output).

## Architecture

```
main.rs           CLI (clap) + orchestration loop; headless poller
git/mod.rs        the only module that shells out to git
git/status.rs     git status --porcelain=v1 -z -> Vec<GitChange>
git/grouping.rs   Vec<GitChange> -> Vec<ChangeGroup> (deterministic order)
git/commit.rs     isolated commit per group, push, commit messages
types.rs          StatusCode, GitStatus, ChangeType
error.rs          GitChaiError (thiserror)
```

Data flow: scan -> group -> commit -> optional push. `git::` is the single shell-out boundary; everything above it is pure orchestration.

## Invariants (do not "fix" these)

- Commits contain exactly the paths they name. `commit_paths` stages into a throwaway index seeded from HEAD and syncs the real index afterwards with `git reset -- <paths>`. Never replace with a plain `git commit -m`: it sweeps the user's pre-staged changes.
- Unmerged paths are never committed (`git ls-files -u` filter).
- Root-level files always commit individually; renames and copies always commit individually; a directory commits as one unit only when every file in it (tracked and untracked) changed with the same change type.
- An empty `RUST_LOG` means unset; the tool must still log in sandboxes.
- Version lives only in `Cargo.toml`; flake and `--version` derive it.
- Filenames are passed with `:(literal)` pathspecs; glob characters are real filenames, not patterns.

## Decisions

- git CLI over libgit2/gix: hooks, attributes/filters, push auth, and `:(literal)` pathspecs come free; process spawns are not a bottleneck at the 5-second poll. Do not migrate without a measured performance need. The functional test suite is the acceptance test for any migration.
- Grouping is deterministic (`BTreeMap` ordering) by design.
- `installer.sh` stays at the repo root: its raw URL is the documented install interface and must not break.

## Code style

- rustfmt and clippy `-D warnings` are the source of truth.
- Errors: thiserror `GitChaiError` inside `git::`, anyhow at the main boundary.
- Log via the `log` crate: info for outcomes, debug for per-file detail.
- No unsafe. No magic strings; prefer enums.

## Testing

- `tests/cli.rs` integration tests run the real binary against scratch repos with git identity set via environment variables; they must pass with no user git config (the nix sandbox proves it).
- `tests/functional.sh <binary>` is the end-to-end suite: 29 scenarios covering every option, grouping rule, and guarantee, including the timing-based headless behavior. It is self-contained (its own scratch repos and git identity), exits non-zero on any failure, and runs in CI on every push. Run it against the flake-built binary before releases.
- Guarantee tests are regressions: if one fails, fix the code, never loosen the test.

## Commits and CI

- One commit per file, subject `mod: <path>` | `add: <path>` | `del: <path>`.
- CI runs fmt, clippy `-D warnings`, `cargo test`, and `tests/functional.sh` on every push; run them locally before pushing.
- Dependency majors are deliberate changes; minor updates via `cargo update`.

## Releases

- Bump the version in `Cargo.toml` only, then verify: full test suite, `nix build`, and `tests/functional.sh` against the flake-built binary.
