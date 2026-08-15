use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::error::GitChaiError;
use crate::git::git_capture;
use crate::git::grouping::FileChange;
use crate::types::ChangeType;

static INDEX_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Stage `paths` into a throwaway index and commit exactly them, then sync
/// the real index for exactly those paths.
///
/// The temporary index is seeded from HEAD, so the commit can never contain
/// anything the user had pre-staged for other paths. The post-commit reset
/// brings the real index up to date for the committed paths only, leaving
/// every other staged entry untouched.
pub fn commit_paths(repo: &Path, paths: &[PathBuf], message: &str) -> Result<(), GitChaiError> {
    log::debug!("Committing {:?} as '{}'", paths, message);

    let git_dir = resolve_git_dir(repo)?;
    let index_path = git_dir.join(format!(
        "index.git-chai.{}.{}",
        std::process::id(),
        INDEX_COUNTER.fetch_add(1, Ordering::SeqCst)
    ));

    let pathspecs: Vec<OsString> = paths
        .iter()
        .map(|p| crate::git::literal_pathspec(p))
        .collect();

    let result = commit_with_temp_index(repo, &index_path, &pathspecs, message);

    match std::fs::remove_file(&index_path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => log::warn!(
            "Failed to remove temporary index {}: {}",
            index_path.display(),
            e
        ),
    }

    result?;

    // Success: make the real index match HEAD for the committed paths.
    let mut reset_args: Vec<&OsStr> = vec![OsStr::new("reset"), OsStr::new("-q"), OsStr::new("--")];
    reset_args.extend(pathspecs.iter().map(|p| p.as_os_str()));
    git_capture(repo, None, &reset_args)?;

    log::debug!("Committed '{}'", message);
    Ok(())
}

fn commit_with_temp_index(
    repo: &Path,
    index_path: &Path,
    pathspecs: &[OsString],
    message: &str,
) -> Result<(), GitChaiError> {
    // Seed the temporary index from HEAD. On an unborn HEAD the rev-parse
    // probe fails and we start from an empty index instead (root commit).
    let has_head = git_capture(
        repo,
        None,
        &[
            OsStr::new("rev-parse"),
            OsStr::new("--verify"),
            OsStr::new("-q"),
            OsStr::new("HEAD"),
        ],
    )
    .is_ok();
    if has_head {
        git_capture(
            repo,
            Some(index_path),
            &[OsStr::new("read-tree"), OsStr::new("HEAD")],
        )?;
    }

    let mut add_args: Vec<&OsStr> = vec![OsStr::new("add"), OsStr::new("--all"), OsStr::new("--")];
    add_args.extend(pathspecs.iter().map(|p| p.as_os_str()));
    git_capture(repo, Some(index_path), &add_args)?;

    git_capture(
        repo,
        Some(index_path),
        &[OsStr::new("commit"), OsStr::new("-m"), OsStr::new(message)],
    )?;
    Ok(())
}

pub fn push_changes(repo: &Path) -> Result<(), GitChaiError> {
    log::debug!("Pushing changes to remote");
    git_capture(
        repo,
        None,
        &[OsStr::new("push"), OsStr::new("origin"), OsStr::new("HEAD")],
    )?;
    log::debug!("Successfully pushed changes to remote");
    Ok(())
}

fn resolve_git_dir(repo: &Path) -> Result<PathBuf, GitChaiError> {
    let raw = git_capture(
        repo,
        None,
        &[OsStr::new("rev-parse"), OsStr::new("--git-dir")],
    )?;
    let dir = PathBuf::from(String::from_utf8_lossy(&raw).trim().to_string());
    Ok(if dir.is_absolute() {
        dir
    } else {
        repo.join(dir)
    })
}

/// Commit message for a whole directory: "mod: src".
pub fn directory_commit_message(dir: &Path, change_type: ChangeType) -> String {
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| dir.to_string_lossy().into_owned());
    format!("{}: {}", change_type, name)
}

/// Commit message for a single file: "mod: src/main.rs", or
/// "rename: src/a.rs -> src/b.rs" for renames and copies.
pub fn file_commit_message(file: &FileChange) -> String {
    if let Some(old) = &file.old_filename {
        match file.change_type {
            ChangeType::Rename => {
                format!("rename: {} -> {}", old.display(), file.filename.display())
            }
            ChangeType::Copy => {
                format!("copy: {} -> {}", old.display(), file.filename.display())
            }
            _ => format!("{}: {}", file.change_type, file.filename.display()),
        }
    } else {
        format!("{}: {}", file.change_type, file.filename.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ChangeType;

    fn file_change(filename: &str, change_type: ChangeType) -> FileChange {
        FileChange {
            filename: PathBuf::from(filename),
            old_filename: None,
            change_type,
        }
    }

    #[test]
    fn builds_file_messages() {
        let f = file_change("src/main.rs", ChangeType::Modify);
        assert_eq!(file_commit_message(&f), "mod: src/main.rs");

        let f = file_change("root.txt", ChangeType::Add);
        assert_eq!(file_commit_message(&f), "add: root.txt");

        let f = file_change("gone.txt", ChangeType::Delete);
        assert_eq!(file_commit_message(&f), "del: gone.txt");

        let mut f = file_change("src/b.rs", ChangeType::Rename);
        f.old_filename = Some(PathBuf::from("src/a.rs"));
        assert_eq!(file_commit_message(&f), "rename: src/a.rs -> src/b.rs");

        let mut f = file_change("copy.rs", ChangeType::Copy);
        f.old_filename = Some(PathBuf::from("orig.rs"));
        assert_eq!(file_commit_message(&f), "copy: orig.rs -> copy.rs");
    }

    #[test]
    fn builds_directory_messages() {
        assert_eq!(
            directory_commit_message(Path::new("src"), ChangeType::Modify),
            "mod: src"
        );
        assert_eq!(
            directory_commit_message(Path::new("newpkg"), ChangeType::Add),
            "add: newpkg"
        );
    }
}
