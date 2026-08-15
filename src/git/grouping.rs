use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::error::GitChaiError;
use crate::git::git_capture;
use crate::git::status::GitChange;
use crate::types::{ChangeType, StatusCode};

/// A changed file, ready to be staged and described in a commit message.
#[derive(Debug, Clone)]
pub struct FileChange {
    pub filename: PathBuf,
    pub old_filename: Option<PathBuf>,
    pub change_type: ChangeType,
}

impl From<GitChange> for FileChange {
    fn from(change: GitChange) -> Self {
        Self {
            filename: change.filename,
            old_filename: change.old_filename,
            change_type: change.change_type,
        }
    }
}

/// One unit of work: either a whole directory committed under a single
/// message, or a set of individually committed files.
#[derive(Debug, Clone)]
pub enum ChangeGroup {
    Directory {
        path: PathBuf,
        change_type: ChangeType,
        files: Vec<FileChange>,
    },
    Individual {
        files: Vec<FileChange>,
    },
}

impl ChangeGroup {
    /// A stable sort key: the directory for directory groups, the first
    /// file's path for individual groups.
    fn sort_key(&self) -> PathBuf {
        match self {
            Self::Directory { path, .. } => path.clone(),
            Self::Individual { files } => files
                .first()
                .map(|f| f.filename.clone())
                .unwrap_or_default(),
        }
    }
}

/// Group changes into directory commits (only when *every* file in the
/// directory changed with the same change type, tracked and untracked alike)
/// and individual file commits otherwise. Output order is deterministic.
pub fn group_changes_by_directory(
    repo_path: &Path,
    changes: Vec<GitChange>,
) -> Result<Vec<ChangeGroup>, GitChaiError> {
    let mut dir_groups: BTreeMap<PathBuf, Vec<FileChange>> = BTreeMap::new();
    let mut result: Vec<ChangeGroup> = Vec::new();

    for change in changes {
        let change_type = change.change_type;
        let is_untracked_dir = change_type == ChangeType::Add
            && change.status.worktree == StatusCode::Untracked
            && change.filename.to_str().is_some_and(|s| s.ends_with('/'));

        if is_untracked_dir {
            // "?? dir/" is one record for the whole untracked directory.
            let path = change.filename.components().as_path().to_path_buf();
            result.push(ChangeGroup::Directory {
                path,
                change_type,
                files: vec![FileChange::from(change)],
            });
            continue;
        }

        let parent = change
            .filename
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        dir_groups
            .entry(parent)
            .or_default()
            .push(FileChange::from(change));
    }

    for (dir, files) in dir_groups {
        let uniform = files.iter().all(|f| f.change_type == files[0].change_type);
        // Root-level files always commit individually: a directory commit for
        // "." would need a message named after the whole repository, which is
        // never informative.
        let directory_commit = dir != Path::new(".")
            && uniform
            && !matches!(files[0].change_type, ChangeType::Rename | ChangeType::Copy)
            && all_files_changed(repo_path, &dir, &files)?;

        if directory_commit {
            result.push(ChangeGroup::Directory {
                path: dir,
                change_type: files[0].change_type,
                files,
            });
        } else {
            result.push(ChangeGroup::Individual { files });
        }
    }

    result.sort_by_key(ChangeGroup::sort_key);
    Ok(result)
}

/// True when the changed set is exactly the set of all files in `dir`
/// (tracked plus untracked-but-not-ignored), so a directory commit message
/// describes the directory accurately and the pathspec cannot sweep in
/// unrelated changes.
fn all_files_changed(
    repo_path: &Path,
    dir: &Path,
    files: &[FileChange],
) -> Result<bool, GitChaiError> {
    let changed: BTreeSet<PathBuf> = files.iter().map(|f| f.filename.clone()).collect();
    let all = get_all_files_in_directory(repo_path, dir)?;
    Ok(changed == all)
}

/// Every file under `dir`, tracked and untracked-but-not-ignored.
fn get_all_files_in_directory(
    repo_path: &Path,
    dir: &Path,
) -> Result<BTreeSet<PathBuf>, GitChaiError> {
    let pathspec = crate::git::literal_pathspec(dir);
    let mut all = BTreeSet::new();
    let mut collect = |raw: Vec<u8>| {
        for field in raw.split(|b| *b == 0) {
            if field.is_empty() {
                continue;
            }
            all.insert(path_from_bytes(field));
        }
    };

    let raw = git_capture(
        repo_path,
        None,
        &[
            OsStr::new("ls-files"),
            OsStr::new("-z"),
            OsStr::new("--"),
            &pathspec,
        ],
    )?;
    collect(raw);

    let raw = git_capture(
        repo_path,
        None,
        &[
            OsStr::new("ls-files"),
            OsStr::new("-z"),
            OsStr::new("--others"),
            OsStr::new("--exclude-standard"),
            OsStr::new("--"),
            &pathspec,
        ],
    )?;
    collect(raw);

    Ok(all)
}

#[cfg(unix)]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec()))
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(repo: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(repo)
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn directory_commit_when_whole_dir_changes() {
        let repo = TempRepo::new();
        git(&repo.path, &["init", "-q", "-b", "main"]);
        repo.write("src/a.rs", "a");
        repo.write("src/b.rs", "b");
        repo.write("docs/readme.md", "r");
        git(&repo.path, &["add", "-A"]);
        git(&repo.path, &["commit", "-qm", "init"]);
        repo.write("src/a.rs", "a2");
        repo.write("src/b.rs", "b2");

        let changes = get_changes(&repo.path);
        let groups = group_changes_by_directory(&repo.path, changes).unwrap();
        assert_eq!(groups.len(), 1);
        match &groups[0] {
            ChangeGroup::Directory {
                path,
                change_type,
                files,
            } => {
                assert_eq!(path, Path::new("src"));
                assert_eq!(*change_type, ChangeType::Modify);
                assert_eq!(files.len(), 2);
            }
            other => panic!("expected directory group, got {:?}", other),
        }
    }

    #[test]
    fn partial_directory_goes_individual() {
        let repo = TempRepo::new();
        git(&repo.path, &["init", "-q", "-b", "main"]);
        repo.write("src/a.rs", "a");
        repo.write("src/b.rs", "b");
        git(&repo.path, &["add", "-A"]);
        git(&repo.path, &["commit", "-qm", "init"]);
        repo.write("src/a.rs", "a2");

        let changes = get_changes(&repo.path);
        let groups = group_changes_by_directory(&repo.path, changes).unwrap();
        assert_eq!(groups.len(), 1);
        match &groups[0] {
            ChangeGroup::Individual { files } => assert_eq!(files.len(), 1),
            other => panic!("expected individual group, got {:?}", other),
        }
    }

    #[test]
    fn count_mismatch_never_triggers_directory_commit() {
        // Regression test: one tracked file plus one untracked root file used
        // to compare equal by *count* and sweep the whole repo into "add: .".
        // The root group must stay individual; only src/ may collapse into a
        // directory commit because every file in it changed.
        let repo = TempRepo::new();
        git(&repo.path, &["init", "-q", "-b", "main"]);
        repo.write("src/a.rs", "a");
        git(&repo.path, &["add", "-A"]);
        git(&repo.path, &["commit", "-qm", "init"]);
        repo.write("root.txt", "untracked");
        repo.write("src/a.rs", "a2");

        let changes = get_changes(&repo.path);
        let groups = group_changes_by_directory(&repo.path, changes).unwrap();
        assert_eq!(groups.len(), 2, "expected two groups, got {:?}", groups);
        // Sorted by sort key: "." before "src".
        match &groups[0] {
            ChangeGroup::Individual { files } => {
                assert_eq!(files.len(), 1);
                assert_eq!(files[0].filename, PathBuf::from("root.txt"));
                assert_eq!(files[0].change_type, ChangeType::Add);
            }
            other => panic!("expected individual root group, got {:?}", other),
        }
        match &groups[1] {
            ChangeGroup::Directory {
                path,
                change_type,
                files,
            } => {
                assert_eq!(path, Path::new("src"));
                assert_eq!(*change_type, ChangeType::Modify);
                assert_eq!(files.len(), 1);
            }
            other => panic!("expected directory src group, got {:?}", other),
        }
    }

    #[test]
    fn untracked_directory_becomes_add_group() {
        let repo = TempRepo::new();
        git(&repo.path, &["init", "-q", "-b", "main"]);
        repo.write("tracked.txt", "t");
        git(&repo.path, &["add", "-A"]);
        git(&repo.path, &["commit", "-qm", "init"]);
        repo.write("newpkg/one.rs", "1");
        repo.write("newpkg/two.rs", "2");

        let changes = get_changes(&repo.path);
        let groups = group_changes_by_directory(&repo.path, changes).unwrap();
        assert_eq!(groups.len(), 1);
        match &groups[0] {
            ChangeGroup::Directory {
                path, change_type, ..
            } => {
                assert_eq!(path, Path::new("newpkg"));
                assert_eq!(*change_type, ChangeType::Add);
            }
            other => panic!("expected directory group, got {:?}", other),
        }
    }

    #[test]
    fn mixed_types_go_individual() {
        let repo = TempRepo::new();
        git(&repo.path, &["init", "-q", "-b", "main"]);
        repo.write("src/a.rs", "a");
        repo.write("src/b.rs", "b");
        git(&repo.path, &["add", "-A"]);
        git(&repo.path, &["commit", "-qm", "init"]);
        repo.write("src/a.rs", "a2");
        repo.remove("src/b.rs");

        let changes = get_changes(&repo.path);
        let groups = group_changes_by_directory(&repo.path, changes).unwrap();
        assert_eq!(groups.len(), 1);
        match &groups[0] {
            ChangeGroup::Individual { files } => {
                assert_eq!(files.len(), 2);
                assert_eq!(files[0].change_type, ChangeType::Modify);
                assert_eq!(files[1].change_type, ChangeType::Delete);
            }
            other => panic!("expected individual group, got {:?}", other),
        }
    }

    #[test]
    fn renames_always_go_individual() {
        let repo = TempRepo::new();
        git(&repo.path, &["init", "-q", "-b", "main"]);
        repo.write("src/a.rs", "a");
        git(&repo.path, &["add", "-A"]);
        git(&repo.path, &["commit", "-qm", "init"]);
        git(&repo.path, &["mv", "src/a.rs", "src/b.rs"]);

        let changes = get_changes(&repo.path);
        let groups = group_changes_by_directory(&repo.path, changes).unwrap();
        assert_eq!(groups.len(), 1);
        match &groups[0] {
            ChangeGroup::Individual { files } => {
                assert_eq!(files[0].change_type, ChangeType::Rename);
                assert_eq!(files[0].filename, PathBuf::from("src/b.rs"));
                assert_eq!(files[0].old_filename, Some(PathBuf::from("src/a.rs")));
            }
            other => panic!("expected individual group, got {:?}", other),
        }
    }

    /// Minimal temp-dir fixture.
    struct TempRepo {
        path: PathBuf,
    }

    impl TempRepo {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "git-chai-grouping-test-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }
        fn write(&self, rel: &str, content: &str) {
            let full = self.path.join(rel);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, content).unwrap();
        }
        fn remove(&self, rel: &str) {
            std::fs::remove_file(self.path.join(rel)).unwrap();
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn get_changes(repo: &Path) -> Vec<GitChange> {
        crate::git::status::get_changed_files(repo).unwrap()
    }
}
