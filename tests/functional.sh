#!/usr/bin/env bash
# End-to-end functional suite for git-chai: every documented option,
# grouping rule, and guarantee, driven against the real binary.
#
# Usage: tests/functional.sh [path-to-binary]
# Default binary: target/debug/git-chai next to this script's repo root.
# Exits non-zero if any assertion fails. Self-contained: creates its own
# scratch repositories under a temp dir and sets git identity via -c flags.
set -u

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BIN="${1:-$REPO_ROOT/target/debug/git-chai}"
# Absolute path: the script cd's to its scratch dir, so a relative BIN
# would stop resolving. Plain-name binaries (on PATH) are left as-is.
if [[ "$BIN" == */* ]]; then
    BIN="$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")"
fi
EXPECTED_VER="$(sed -n 's/^version = "\(.*\)"/\1/p' "$REPO_ROOT/Cargo.toml" | head -1)"

ROOT="$(mktemp -d /tmp/git-chai-functional.XXXXXX)"
trap 'rm -rf "$ROOT"' EXIT

PASS=0; FAIL=0
BG_PIDS=()

hdr()  { echo; echo "===== $1 ====="; }
ok()   { PASS=$((PASS+1)); echo "  PASS: $1"; }
bad()  { FAIL=$((FAIL+1)); echo "  FAIL: $1"; }
check(){ if [[ "$2" == "$3" ]]; then ok "$1"; else bad "$1 (got [$2], want [$3])"; fi; }
count_commits(){ git -C "$1" rev-list --count HEAD; }

GITC=(git -c user.name=ChaiTest -c user.email=chai@test.local)

# Identity for every git process the *tool* spawns: the suite must pass on
# machines with no user git config (e.g. GitHub runners), where git commit
# would otherwise fail with "Author identity unknown". The -c flags above
# only cover this script's own git calls.
export GIT_AUTHOR_NAME=ChaiTest GIT_AUTHOR_EMAIL=chai@test.local
export GIT_COMMITTER_NAME=ChaiTest GIT_COMMITTER_EMAIL=chai@test.local

mkdir -p "$ROOT/remotes"
cd "$ROOT"

# ---------------------------------------------------------------------------
hdr "S1: --version (-?) and --version"
out="$("$BIN" -?)"; rc=$?
check "version output" "$out" "git-chai $EXPECTED_VER"; check "version exit 0" "$rc" "0"
out="$("$BIN" --version)"; rc=$?
check "long version exit 0" "$rc" "0"; check "long version output" "$out" "git-chai $EXPECTED_VER"

# ---------------------------------------------------------------------------
hdr "S2: --help"
"$BIN" --help > "$ROOT/help.txt" 2>&1; rc=$?
check "--help exit 0" "$rc" "0"
grep -q -- "--headless" "$ROOT/help.txt" && ok "help lists --headless" || bad "help lists --headless"

# ---------------------------------------------------------------------------
hdr "S3: clean repo is a no-op (exit 0, no commits)"
mkdir -p repo-clean
"${GITC[@]}" -C repo-clean init -q -b main
echo x > repo-clean/a.txt
"${GITC[@]}" -C repo-clean add -A
"${GITC[@]}" -C repo-clean commit -qm init
out="$("$BIN" --repo-path repo-clean 2>&1)"; rc=$?
check "exit 0" "$rc" "0"
echo "$out" | grep -q "No changes detected" && ok "reports no changes" || bad "reports no changes"
check "commit count stays 1" "$(count_commits repo-clean)" "1"

# ---------------------------------------------------------------------------
hdr "S4: basic commit (modify a root file)"
echo changed >> repo-clean/a.txt
out="$("$BIN" --repo-path repo-clean 2>&1)"; rc=$?
check "exit 0" "$rc" "0"
check "commit subject" "$(git -C repo-clean log -1 --format=%s)" "mod: a.txt"
check "commit contains only a.txt" "$(git -C repo-clean show --name-only --format= HEAD)" "a.txt"
check "status clean after" "$(git -C repo-clean status --porcelain)" ""
check "commit count is 2" "$(count_commits repo-clean)" "2"

# ---------------------------------------------------------------------------
hdr "S5: --dry-run commits nothing"
echo again >> repo-clean/a.txt
out="$("$BIN" --repo-path repo-clean --dry-run 2>&1)"; rc=$?
check "exit 0" "$rc" "0"
echo "$out" | grep -q "DRY RUN" && ok "plan shown" || bad "plan shown"
check "no new commit" "$(count_commits repo-clean)" "2"
check "file still modified" "$(git -C repo-clean status --porcelain)" " M a.txt"

# ---------------------------------------------------------------------------
hdr "S6: --verbose emits debug detail"
out="$("$BIN" --repo-path repo-clean --verbose 2>&1)"; rc=$?
check "exit 0" "$rc" "0"
echo "$out" | grep -q "Detected change" && ok "debug scan shown" || bad "debug scan shown"
check "commit subject" "$(git -C repo-clean log -1 --format=%s)" "mod: a.txt"
check "status clean after" "$(git -C repo-clean status --porcelain)" ""

# ---------------------------------------------------------------------------
hdr "S7: --dry-run --verbose lists files without committing"
echo one >> repo-clean/a.txt
out="$("$BIN" --repo-path repo-clean --dry-run --verbose 2>&1)"; rc=$?
check "exit 0" "$rc" "0"
echo "$out" | grep -q "DRY RUN: would commit 'mod: a.txt'" && ok "exact plan shown" || bad "exact plan shown"
check "no new commit" "$(count_commits repo-clean)" "3"

# ---------------------------------------------------------------------------
hdr "S8: --repo-path from an unrelated directory"
mkdir -p elsewhere/deep
(cd elsewhere/deep && "$BIN" --repo-path "$ROOT/repo-clean" > "$ROOT/s8.out" 2>&1); rc=$?
check "exit 0" "$rc" "0"
check "commit subject" "$(git -C repo-clean log -1 --format=%s)" "mod: a.txt"
check "status clean after" "$(git -C repo-clean status --porcelain)" ""
check "commit count is 4" "$(count_commits repo-clean)" "4"

# ---------------------------------------------------------------------------
hdr "S9: directory commit when every file in dir changed"
mkdir -p repo-dir/src repo-dir/docs
echo a > repo-dir/src/a.rs; echo b > repo-dir/src/b.rs; echo r > repo-dir/docs/readme.md
"${GITC[@]}" -C repo-dir init -q -b main
"${GITC[@]}" -C repo-dir add -A
"${GITC[@]}" -C repo-dir commit -qm init
echo a2 >> repo-dir/src/a.rs; echo b2 >> repo-dir/src/b.rs
out="$("$BIN" --repo-path repo-dir 2>&1)"; rc=$?
check "exit 0" "$rc" "0"
check "commit subject" "$(git -C repo-dir log -1 --format=%s)" "mod: src"
check "both files committed" "$(git -C repo-dir show --name-only --format= HEAD | sort | tr '\n' ' ')" "src/a.rs src/b.rs "
check "docs untouched, clean status" "$(git -C repo-dir status --porcelain)" ""

# ---------------------------------------------------------------------------
hdr "S10: partial directory change commits the file individually"
echo a3 >> repo-dir/src/a.rs
out="$("$BIN" --repo-path repo-dir 2>&1)"; rc=$?
check "exit 0" "$rc" "0"
check "commit subject" "$(git -C repo-dir log -1 --format=%s)" "mod: src/a.rs"

# ---------------------------------------------------------------------------
hdr "S11: mixed change types in one directory commit individually"
echo m >> repo-dir/src/a.rs
rm repo-dir/src/b.rs
out="$("$BIN" --repo-path repo-dir 2>&1)"; rc=$?
check "exit 0" "$rc" "0"
subj2="$(git -C repo-dir log -2 --format=%s | sort | tr '\n' '|')"
echo "$subj2" | grep -q "mod: src/a.rs" && echo "$subj2" | grep -q "del: src/b.rs" \
  && ok "both individual commits" || bad "both individual commits (got [$subj2])"

# ---------------------------------------------------------------------------
hdr "S12: rename commits with old -> new message"
mkdir -p repo-ren/src
echo a > repo-ren/src/a.rs
"${GITC[@]}" -C repo-ren init -q -b main
"${GITC[@]}" -C repo-ren add -A
"${GITC[@]}" -C repo-ren commit -qm init
"${GITC[@]}" -C repo-ren mv src/a.rs src/b.rs
out="$("$BIN" --repo-path repo-ren 2>&1)"; rc=$?
check "exit 0" "$rc" "0"
check "commit subject" "$(git -C repo-ren log -1 --format=%s)" "rename: src/a.rs -> src/b.rs"
check "new path committed, old gone" "$(git -C repo-ren show --name-only --format= HEAD)" "src/b.rs"
check "status clean after" "$(git -C repo-ren status --porcelain)" ""

# ---------------------------------------------------------------------------
hdr "S13: copy: git emits no C record for this case; copy path is unit-tested"
cp repo-ren/src/b.rs repo-ren/src/c.rs
out="$("$BIN" --repo-path repo-ren 2>&1)"; rc=$?
check "exit 0" "$rc" "0"
check "copied file commits as add" "$(git -C repo-ren log -1 --format=%s)" "add: src/c.rs"

# ---------------------------------------------------------------------------
hdr "S14: untracked directory commits as one add"
mkdir -p repo-ren/newpkg
echo 1 > repo-ren/newpkg/one.rs; echo 2 > repo-ren/newpkg/two.rs
out="$("$BIN" --repo-path repo-ren 2>&1)"; rc=$?
check "exit 0" "$rc" "0"
check "commit subject" "$(git -C repo-ren log -1 --format=%s)" "add: newpkg"
check "both files committed" "$(git -C repo-ren show --name-only --format= HEAD | sort | tr '\n' ' ')" "newpkg/one.rs newpkg/two.rs "

# ---------------------------------------------------------------------------
hdr "S15: deletion commits as del"
mkdir -p repo-del
echo g > repo-del/gone.txt; echo k > repo-del/keep.txt
"${GITC[@]}" -C repo-del init -q -b main
"${GITC[@]}" -C repo-del add -A
"${GITC[@]}" -C repo-del commit -qm init
rm repo-del/gone.txt
out="$("$BIN" --repo-path repo-del 2>&1)"; rc=$?
check "exit 0" "$rc" "0"
check "commit subject" "$(git -C repo-del log -1 --format=%s)" "del: gone.txt"
check "keep.txt untouched" "$(git -C repo-del status --porcelain)" ""

# ---------------------------------------------------------------------------
hdr "S16: pre-staged changes are never swept into another commit"
mkdir -p repo-pre/src
echo a > repo-pre/src/a.rs
"${GITC[@]}" -C repo-pre init -q -b main
"${GITC[@]}" -C repo-pre add -A
"${GITC[@]}" -C repo-pre commit -qm init
echo a2 >> repo-pre/src/a.rs
echo user > repo-pre/user-file.txt
"${GITC[@]}" -C repo-pre add user-file.txt
out="$("$BIN" --repo-path repo-pre 2>&1)"; rc=$?
check "exit 0" "$rc" "0"
check "HEAD is the pre-staged commit" "$(git -C repo-pre log -1 --format=%s)" "add: user-file.txt"
check "pre-staged commit contains only its file" "$(git -C repo-pre show --name-only --format= HEAD)" "user-file.txt"
check "mod commit contains only a.rs" "$(git -C repo-pre show --name-only --format= HEAD~1)" "src/a.rs"
# src/ holds a single file, so every file in it changed: directory commit.
check "mod commit subject correct" "$(git -C repo-pre log -1 --format=%s HEAD~1)" "mod: src"
check "status clean after" "$(git -C repo-pre status --porcelain)" ""

# ---------------------------------------------------------------------------
hdr "S17: unmerged paths are skipped, never committed"
mkdir -p repo-conflict
echo base > repo-conflict/a.txt
"${GITC[@]}" -C repo-conflict init -q -b main
"${GITC[@]}" -C repo-conflict add -A
"${GITC[@]}" -C repo-conflict commit -qm init
"${GITC[@]}" -C repo-conflict checkout -qb feature
echo feature > repo-conflict/a.txt
"${GITC[@]}" -C repo-conflict commit -qam "feature change"
"${GITC[@]}" -C repo-conflict checkout -q main
echo main > repo-conflict/a.txt
"${GITC[@]}" -C repo-conflict commit -qam "main change"
"${GITC[@]}" -C repo-conflict merge feature >/dev/null 2>&1
out="$("$BIN" --repo-path repo-conflict 2>&1)"; rc=$?
check "exit 0" "$rc" "0"
echo "$out" | grep -q "Skipping unmerged path" && ok "skip warning shown" || bad "skip warning shown"
check "no new commit on main" "$(count_commits repo-conflict)" "2"
git -C repo-conflict status --porcelain | grep -q "^UU a.txt" && ok "still unmerged" || bad "still unmerged"

# ---------------------------------------------------------------------------
hdr "S18: failed commit yields non-zero exit"
mkdir -p repo-hook
echo h > repo-hook/a.txt
"${GITC[@]}" -C repo-hook init -q -b main
"${GITC[@]}" -C repo-hook add -A
"${GITC[@]}" -C repo-hook commit -qm init
printf '#!/bin/sh\nexit 1\n' > repo-hook/.git/hooks/pre-commit
chmod +x repo-hook/.git/hooks/pre-commit
echo h2 >> repo-hook/a.txt
out="$("$BIN" --repo-path repo-hook 2>&1)"; rc=$?
check "exit non-zero" "$rc" "1"
echo "$out" | grep -q "failed to commit" && ok "failure reported" || bad "failure reported"
check "nothing committed" "$(count_commits repo-hook)" "1"

# ---------------------------------------------------------------------------
hdr "S19: --push advances the remote"
mkdir -p repo-push
echo p > repo-push/a.txt
"${GITC[@]}" -C repo-push init -q -b main
"${GITC[@]}" -C repo-push add -A
"${GITC[@]}" -C repo-push commit -qm init
"${GITC[@]}" -C repo-push init -q --bare "$ROOT/remotes/push.git"
"${GITC[@]}" -C repo-push remote add origin "$ROOT/remotes/push.git"
"${GITC[@]}" -C repo-push push -q -u origin main
echo p2 >> repo-push/a.txt
out="$("$BIN" --repo-path repo-push --push 2>&1)"; rc=$?
check "exit 0" "$rc" "0"
echo "$out" | grep -q "Successfully pushed" && ok "push reported" || bad "push reported"
check "remote advanced" "$(git -C "$ROOT/remotes/push.git" rev-parse main)" "$(git -C repo-push rev-parse main)"

# ---------------------------------------------------------------------------
hdr "S20: --push --verbose"
echo p3 >> repo-push/a.txt
out="$("$BIN" --repo-path repo-push --push --verbose 2>&1)"; rc=$?
check "exit 0" "$rc" "0"
echo "$out" | grep -q "Detected change" && ok "verbose detail shown" || bad "verbose detail shown"
check "remote advanced" "$(git -C "$ROOT/remotes/push.git" rev-parse main)" "$(git -C repo-push rev-parse main)"

# ---------------------------------------------------------------------------
hdr "S21: --repo-path --push from another directory"
echo p4 >> repo-push/a.txt
(cd elsewhere/deep && "$BIN" --repo-path "$ROOT/repo-push" --push > "$ROOT/s21.out" 2>&1); rc=$?
check "exit 0" "$rc" "0"
check "remote advanced" "$(git -C "$ROOT/remotes/push.git" rev-parse main)" "$(git -C repo-push rev-parse main)"

# ---------------------------------------------------------------------------
hdr "S22: --push with no remote: commit lands, push warns, exit 0"
mkdir -p repo-noremote
echo n > repo-noremote/a.txt
"${GITC[@]}" -C repo-noremote init -q -b main
"${GITC[@]}" -C repo-noremote add -A
"${GITC[@]}" -C repo-noremote commit -qm init
echo n2 >> repo-noremote/a.txt
out="$("$BIN" --repo-path repo-noremote --push 2>&1)"; rc=$?
check "exit 0" "$rc" "0"
echo "$out" | grep -q "Failed to push" && ok "push failure warned" || bad "push failure warned"
check "commit landed locally" "$(count_commits repo-noremote)" "2"

# ---------------------------------------------------------------------------
hdr "S23: --headless commits changes made while running (Ctrl+C stops)"
mkdir -p repo-headless
echo h > repo-headless/a.txt
"${GITC[@]}" -C repo-headless init -q -b main
"${GITC[@]}" -C repo-headless add -A
"${GITC[@]}" -C repo-headless commit -qm init
( sleep 3; echo h2 >> repo-headless/a.txt ) &
BG_PIDS+=($!)
out="$(timeout --signal=INT -k 5 15 "$BIN" --repo-path repo-headless --headless 2>&1)"; rc=$?
echo "$out" | grep -q "Starting in headless mode" && ok "headless start shown" || bad "headless start shown"
check "change auto-committed" "$(git -C repo-headless log -1 --format=%s)" "mod: a.txt"
echo "$out" | grep -q "Received interrupt signal" && ok "Ctrl+C handled gracefully" || bad "Ctrl+C handled gracefully"
echo "$out" | grep -q "git-chai stopped" && ok "clean shutdown logged" || bad "clean shutdown logged"
if [[ "$rc" == "0" || "$rc" == "124" ]]; then ok "exit code acceptable ($rc)"; else bad "exit code acceptable (got $rc)"; fi

# ---------------------------------------------------------------------------
hdr "S24: --headless --push keeps the remote in sync"
mkdir -p repo-hlpush
echo h > repo-hlpush/a.txt
"${GITC[@]}" -C repo-hlpush init -q -b main
"${GITC[@]}" -C repo-hlpush add -A
"${GITC[@]}" -C repo-hlpush commit -qm init
"${GITC[@]}" -C repo-hlpush init -q --bare "$ROOT/remotes/hlpush.git"
"${GITC[@]}" -C repo-hlpush remote add origin "$ROOT/remotes/hlpush.git"
"${GITC[@]}" -C repo-hlpush push -q -u origin main
( sleep 3; echo h2 >> repo-hlpush/a.txt ) &
BG_PIDS+=($!)
out="$(timeout --signal=INT -k 5 15 "$BIN" --repo-path repo-hlpush --headless --push 2>&1)"; rc=$?
check "commit made" "$(git -C repo-hlpush log -1 --format=%s)" "mod: a.txt"
check "remote advanced" "$(git -C "$ROOT/remotes/hlpush.git" rev-parse main)" "$(git -C repo-hlpush rev-parse main)"
echo "$out" | grep -q "Successfully pushed" && ok "push reported" || bad "push reported"

# ---------------------------------------------------------------------------
hdr "S25: --headless --push --verbose"
mkdir -p repo-hlv
echo h > repo-hlv/a.txt
"${GITC[@]}" -C repo-hlv init -q -b main
"${GITC[@]}" -C repo-hlv add -A
"${GITC[@]}" -C repo-hlv commit -qm init
"${GITC[@]}" -C repo-hlv init -q --bare "$ROOT/remotes/hlv.git"
"${GITC[@]}" -C repo-hlv remote add origin "$ROOT/remotes/hlv.git"
"${GITC[@]}" -C repo-hlv push -q -u origin main
( sleep 3; echo h2 >> repo-hlv/a.txt ) &
BG_PIDS+=($!)
out="$(timeout --signal=INT -k 5 15 "$BIN" --repo-path repo-hlv --headless --push --verbose 2>&1)"; rc=$?
echo "$out" | grep -q "Detected change" && ok "verbose detail shown" || bad "verbose detail shown"
check "remote advanced" "$(git -C "$ROOT/remotes/hlv.git" rev-parse main)" "$(git -C repo-hlv rev-parse main)"

# ---------------------------------------------------------------------------
hdr "S26: fresh repository gets a first commit"
mkdir -p repo-fresh
echo f > repo-fresh/a.txt
"${GITC[@]}" -C repo-fresh init -q -b main
out="$("$BIN" --repo-path repo-fresh 2>&1)"; rc=$?
check "exit 0" "$rc" "0"
check "root commit subject" "$(git -C repo-fresh log -1 --format=%s)" "add: a.txt"
check "commit count 1" "$(count_commits repo-fresh)" "1"

# ---------------------------------------------------------------------------
hdr "S27: glob characters and spaces in filenames"
mkdir -p repo-odd
echo w > 'repo-odd/we[ird].txt'
echo s > 'repo-odd/dir with space.txt'
"${GITC[@]}" -C repo-odd init -q -b main
"${GITC[@]}" -C repo-odd add -A
"${GITC[@]}" -C repo-odd commit -qm init
echo w2 >> 'repo-odd/we[ird].txt'
echo s2 >> 'repo-odd/dir with space.txt'
out="$("$BIN" --repo-path repo-odd 2>&1)"; rc=$?
check "exit 0" "$rc" "0"
subj="$(git -C repo-odd log -2 --format=%s | sort | tr '\n' '|')"
echo "$subj" | grep -q "mod: we\[ird\].txt" && echo "$subj" | grep -q "mod: dir with space.txt" \
  && ok "both odd names committed" || bad "both odd names committed (got [$subj])"
check "status clean after" "$(git -C repo-odd status --porcelain)" ""

# ---------------------------------------------------------------------------
hdr "S28: not a git repository fails clearly"
mkdir -p plain-dir
echo x > plain-dir/file.txt
out="$("$BIN" --repo-path plain-dir 2>&1)"; rc=$?
check "exit non-zero" "$rc" "1"
echo "$out" | grep -qi "failed to resolve" && ok "clear error shown" || bad "clear error shown (got [$out])"

# ---------------------------------------------------------------------------
hdr "S29: --repo-path pointing at a non-repo fails"
out="$("$BIN" --repo-path /tmp 2>&1)"; rc=$?
check "exit non-zero" "$rc" "1"

echo
echo "=========================================="
echo "TOTAL: pass $PASS, fail $FAIL"
echo "=========================================="
# Let any background writers finish before the cleanup trap removes ROOT.
for pid in "${BG_PIDS[@]}"; do wait "$pid" 2>/dev/null; done
exit $((FAIL > 0))
