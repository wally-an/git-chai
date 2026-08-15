//! End-to-end tests: run the real binary against scratch repositories and
//! assert on the resulting commit history and index state.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_git-chai");

/// Identity env vars applied to every git invocation and to git-chai itself,
/// so tests never depend on user config.
fn git_envs(cmd: &mut Command) {
    cmd.env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", std::env::temp_dir());
}

fn git(repo: &Path, args: &[&str]) -> String {
    let mut cmd = Command::new("git");
    cmd.current_dir(repo);
    git_envs(&mut cmd);
    let out = cmd.args(args).output().expect("failed to run git");
    assert!(
        out.status.success(),
        "git {:?} failed: {}\nstdout: {}",
        args,
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn run_chai(repo: &Path, args: &[&str]) -> (bool, String) {
    let mut cmd = Command::new(BIN);
    cmd.current_dir(repo);
    git_envs(&mut cmd);
    let out = cmd.args(args).output().expect("failed to run git-chai");
    let mut output = String::from_utf8_lossy(&out.stdout).into_owned();
    output.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), output)
}

struct Repo {
    path: PathBuf,
}

impl Repo {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "git-chai-cli-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        let repo = Self { path };
        git(&repo.path, &["init", "-q", "-b", "main"]);
        repo
    }

    fn write(&self, rel: &str, content: &str) {
        let full = self.path.join(rel);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }

    fn commit_all(&self, message: &str) {
        git(&self.path, &["add", "-A"]);
        git(&self.path, &["commit", "-qm", message]);
    }

    /// Commit history, oldest first, each as (subject, files changed).
    fn history(&self) -> Vec<(String, Vec<String>)> {
        let hashes: Vec<String> = git(&self.path, &["log", "--format=%H"])
            .lines()
            .map(|l| l.to_string())
            .collect();
        hashes
            .into_iter()
            .rev()
            .map(|h| {
                let subject = git(&self.path, &["log", "-1", "--format=%s", &h])
                    .trim()
                    .to_string();
                let files: Vec<String> = git(&self.path, &["show", "--name-only", "--format=", &h])
                    .lines()
                    .map(|l| l.to_string())
                    .collect();
                (subject, files)
            })
            .collect()
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn directory_commit_when_whole_dir_changes() {
    let repo = Repo::new();
    repo.write("src/a.rs", "a");
    repo.write("src/b.rs", "b");
    repo.write("docs/readme.md", "r");
    repo.commit_all("init");

    repo.write("src/a.rs", "a2");
    repo.write("src/b.rs", "b2");

    let (ok, _) = run_chai(&repo.path, &[]);
    assert!(ok);

    let history = repo.history();
    assert_eq!(history.len(), 2);
    let (subject, files) = &history[1];
    assert_eq!(subject, "mod: src");
    assert_eq!(files.as_slice(), ["src/a.rs", "src/b.rs"]);
    assert_eq!(git(&repo.path, &["status", "--porcelain"]), "");
}

#[test]
fn partial_directory_change_commits_individual_file() {
    let repo = Repo::new();
    repo.write("src/a.rs", "a");
    repo.write("src/b.rs", "b");
    repo.commit_all("init");

    repo.write("src/a.rs", "a2");

    let (ok, _) = run_chai(&repo.path, &[]);
    assert!(ok);

    let history = repo.history();
    assert_eq!(history.len(), 2);
    let (subject, files) = &history[1];
    assert_eq!(subject, "mod: src/a.rs");
    assert_eq!(files.as_slice(), ["src/a.rs"]);
}

#[test]
fn untracked_root_file_never_sweeps_whole_repo() {
    // Regression test for the "add: ." whole-repo sweep: one tracked file,
    // one untracked root file, plus a modification elsewhere.
    let repo = Repo::new();
    repo.write("src/a.rs", "a");
    repo.commit_all("init");

    repo.write("root.txt", "untracked");
    repo.write("src/a.rs", "a2");

    let (ok, _) = run_chai(&repo.path, &[]);
    assert!(ok);

    let history = repo.history();
    assert_eq!(history.len(), 3, "history: {:?}", history);
    let (subject, files) = &history[1];
    assert_eq!(subject, "add: root.txt");
    assert_eq!(files.as_slice(), ["root.txt"]);
    let (subject, files) = &history[2];
    assert_eq!(subject, "mod: src");
    assert_eq!(files.as_slice(), ["src/a.rs"]);
    assert_eq!(git(&repo.path, &["status", "--porcelain"]), "");
}

#[test]
fn prestaged_changes_are_never_swept_into_other_commits() {
    let repo = Repo::new();
    repo.write("a.txt", "a");
    repo.commit_all("init");

    // The user modifies a.txt but also pre-stages an unrelated new file.
    repo.write("a.txt", "a2");
    repo.write("staged-by-user.txt", "user content");
    git(&repo.path, &["add", "staged-by-user.txt"]);

    let (ok, _) = run_chai(&repo.path, &[]);
    assert!(ok);

    let history = repo.history();
    assert_eq!(history.len(), 3, "history: {:?}", history);
    // Each commit contains exactly the files its message names; the
    // pre-staged file gets its own accurate commit instead of being swept
    // into "mod: a.txt".
    let (subject, files) = &history[1];
    assert_eq!(subject, "mod: a.txt");
    assert_eq!(files.as_slice(), ["a.txt"]);
    let (subject, files) = &history[2];
    assert_eq!(subject, "add: staged-by-user.txt");
    assert_eq!(files.as_slice(), ["staged-by-user.txt"]);
    assert_eq!(git(&repo.path, &["status", "--porcelain"]), "");
}

#[test]
fn renames_commit_with_old_and_new_path() {
    let repo = Repo::new();
    repo.write("src/a.rs", "a");
    repo.commit_all("init");

    git(&repo.path, &["mv", "src/a.rs", "src/b.rs"]);

    let (ok, _) = run_chai(&repo.path, &[]);
    assert!(ok);

    let history = repo.history();
    assert_eq!(history.len(), 2, "history: {:?}", history);
    let (subject, files) = &history[1];
    assert_eq!(subject, "rename: src/a.rs -> src/b.rs");
    assert_eq!(files.as_slice(), ["src/b.rs"]);
    assert_eq!(git(&repo.path, &["status", "--porcelain"]), "");
}

#[test]
fn deletions_commit() {
    let repo = Repo::new();
    repo.write("gone.txt", "g");
    repo.commit_all("init");

    std::fs::remove_file(repo.path.join("gone.txt")).unwrap();

    let (ok, _) = run_chai(&repo.path, &[]);
    assert!(ok);

    let history = repo.history();
    assert_eq!(history.len(), 2, "history: {:?}", history);
    let (subject, files) = &history[1];
    assert_eq!(subject, "del: gone.txt");
    assert_eq!(files.as_slice(), ["gone.txt"]);
}

#[test]
fn untracked_directory_commits_as_one() {
    let repo = Repo::new();
    repo.write("tracked.txt", "t");
    repo.commit_all("init");

    repo.write("newpkg/one.rs", "1");
    repo.write("newpkg/two.rs", "2");

    let (ok, _) = run_chai(&repo.path, &[]);
    assert!(ok);

    let history = repo.history();
    assert_eq!(history.len(), 2, "history: {:?}", history);
    let (subject, files) = &history[1];
    assert_eq!(subject, "add: newpkg");
    assert_eq!(files.as_slice(), ["newpkg/one.rs", "newpkg/two.rs"]);
}

#[test]
fn mixed_change_types_commit_individually() {
    let repo = Repo::new();
    repo.write("src/a.rs", "a");
    repo.write("src/b.rs", "b");
    repo.commit_all("init");

    repo.write("src/a.rs", "a2");
    std::fs::remove_file(repo.path.join("src/b.rs")).unwrap();

    let (ok, _) = run_chai(&repo.path, &[]);
    assert!(ok);

    let history = repo.history();
    assert_eq!(history.len(), 3, "history: {:?}", history);
    // Individual files commit in status order (alphabetical).
    let (subject, files) = &history[1];
    assert_eq!(subject, "mod: src/a.rs");
    assert_eq!(files.as_slice(), ["src/a.rs"]);
    let (subject, files) = &history[2];
    assert_eq!(subject, "del: src/b.rs");
    assert_eq!(files.as_slice(), ["src/b.rs"]);
}

#[test]
fn dry_run_commits_nothing() {
    let repo = Repo::new();
    repo.write("a.txt", "a");
    repo.commit_all("init");

    repo.write("a.txt", "a2");

    let (ok, output) = run_chai(&repo.path, &["--dry-run"]);
    assert!(ok);
    assert!(output.contains("DRY RUN"));

    let history = repo.history();
    assert_eq!(history.len(), 1, "history: {:?}", history);
    assert_eq!(git(&repo.path, &["status", "--porcelain"]), " M a.txt\n");
}

#[test]
fn unmerged_paths_are_skipped() {
    let repo = Repo::new();
    repo.write("a.txt", "base\n");
    repo.commit_all("init");

    git(&repo.path, &["checkout", "-q", "-b", "feature"]);
    repo.write("a.txt", "feature\n");
    repo.commit_all("feature change");
    git(&repo.path, &["checkout", "-q", "main"]);
    repo.write("a.txt", "main\n");
    repo.commit_all("main change");

    // Conflict: exit status is non-zero, that is expected.
    let mut cmd = Command::new("git");
    cmd.current_dir(&repo.path);
    git_envs(&mut cmd);
    let _ = cmd.args(["merge", "feature"]).output().unwrap();

    let (ok, output) = run_chai(&repo.path, &[]);
    assert!(ok);
    assert!(
        output.contains("Skipping unmerged path"),
        "output: {}",
        output
    );

    let history = repo.history();
    assert_eq!(history.len(), 2, "history: {:?}", history);
    // The conflicted path is still unmerged, nothing was committed.
    assert!(git(&repo.path, &["status", "--porcelain"]).contains("UU a.txt"));
}

#[test]
fn glob_chars_in_filenames_commit() {
    let repo = Repo::new();
    repo.write("we[ird].txt", "w");
    repo.commit_all("init");

    repo.write("we[ird].txt", "w2");

    let (ok, _) = run_chai(&repo.path, &[]);
    assert!(ok);

    let history = repo.history();
    assert_eq!(history.len(), 2, "history: {:?}", history);
    let (subject, files) = &history[1];
    assert_eq!(subject, "mod: we[ird].txt");
    assert_eq!(files.as_slice(), ["we[ird].txt"]);
}

#[test]
fn filenames_with_spaces_commit() {
    let repo = Repo::new();
    repo.write("dir/with space.txt", "s");
    repo.write("dir/untouched.txt", "u");
    repo.commit_all("init");

    repo.write("dir/with space.txt", "s2");

    let (ok, _) = run_chai(&repo.path, &[]);
    assert!(ok);

    let history = repo.history();
    assert_eq!(history.len(), 2, "history: {:?}", history);
    let (subject, files) = &history[1];
    assert_eq!(subject, "mod: dir/with space.txt");
    assert_eq!(files.as_slice(), ["dir/with space.txt"]);
}

#[test]
fn clean_repo_is_a_noop() {
    let repo = Repo::new();
    repo.write("a.txt", "a");
    repo.commit_all("init");

    let (ok, output) = run_chai(&repo.path, &[]);
    assert!(ok);
    assert!(output.contains("No changes detected"));

    assert_eq!(repo.history().len(), 1);
}

#[test]
fn fresh_repo_with_one_file_gets_root_commit() {
    let repo = Repo::new();
    repo.write("a.txt", "a");

    let (ok, _) = run_chai(&repo.path, &[]);
    assert!(ok);

    let history = repo.history();
    assert_eq!(history.len(), 1, "history: {:?}", history);
    let (subject, files) = &history[0];
    assert_eq!(subject, "add: a.txt");
    assert_eq!(files.as_slice(), ["a.txt"]);
}

#[test]
fn push_advances_remote() {
    let repo = Repo::new();
    repo.write("a.txt", "a");
    repo.commit_all("init");

    let bare = std::env::temp_dir().join(format!(
        "git-chai-bare-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&bare);
    git(
        &repo.path,
        &["init", "-q", "--bare", bare.to_str().unwrap()],
    );
    git(
        &repo.path,
        &["remote", "add", "origin", bare.to_str().unwrap()],
    );
    git(&repo.path, &["push", "-q", "-u", "origin", "main"]);

    repo.write("a.txt", "a2");

    let (ok, _) = run_chai(&repo.path, &["--push"]);
    assert!(ok);

    let local_head = git(&repo.path, &["rev-parse", "main"]).trim().to_string();
    let remote_head = git(&bare, &["rev-parse", "main"]).trim().to_string();
    assert_eq!(local_head, remote_head);

    let _ = std::fs::remove_dir_all(&bare);
}
