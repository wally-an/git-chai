use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::error::GitChaiError;
use crate::git::git_capture;
use crate::types::{ChangeType, GitStatus, StatusCode};

/// One changed path with enough information to stage and describe it.
#[derive(Debug, Clone)]
pub struct GitChange {
    pub status: GitStatus,
    pub change_type: ChangeType,
    /// The path to stage: for renames and copies this is the destination.
    pub filename: PathBuf,
    /// For renames and copies: the source path, also staged (as a deletion
    /// for renames, unchanged for copies).
    pub old_filename: Option<PathBuf>,
}

/// A parsed porcelain record, before classification into a change type.
#[derive(Debug, Clone)]
struct RawChange {
    status: GitStatus,
    filename: PathBuf,
    old_filename: Option<PathBuf>,
}

pub fn get_changed_files(repo_path: &Path) -> Result<Vec<GitChange>, GitChaiError> {
    log::debug!("Scanning for changes in {:?}", repo_path);

    let raw = git_capture(
        repo_path,
        None,
        &[
            OsStr::new("status"),
            OsStr::new("--porcelain=v1"),
            OsStr::new("-z"),
        ],
    )?;
    let unmerged = unmerged_paths(repo_path)?;

    let mut changes = Vec::new();
    for entry in parse_porcelain(&raw)? {
        // Skip paths with unmerged index entries (conflict states such as
        // UU, AA, or DD): auto-committing those would record conflict
        // markers. The ls-files -u probe catches states without a 'U'
        // letter (AA, DD); the status check catches the rest.
        if unmerged.contains(&entry.filename)
            || entry.status.index == StatusCode::Unmerged
            || entry.status.worktree == StatusCode::Unmerged
        {
            log::warn!("Skipping unmerged path: {}", entry.filename.display());
            continue;
        }

        match ChangeType::from_status(&entry.status) {
            Some(change_type) => {
                log::debug!(
                    "Detected change: {} - {}",
                    entry.status,
                    entry.filename.display()
                );
                changes.push(GitChange {
                    status: entry.status,
                    change_type,
                    filename: entry.filename,
                    old_filename: entry.old_filename,
                });
            }
            None => log::debug!("Skipping ignored path: {}", entry.filename.display()),
        }
    }

    log::debug!("Found {} changed file(s)", changes.len());
    Ok(changes)
}

/// Paths with unmerged index entries (stage 1-3). These are conflict states
/// and must never be auto-committed.
fn unmerged_paths(repo_path: &Path) -> Result<BTreeSet<PathBuf>, GitChaiError> {
    let raw = git_capture(
        repo_path,
        None,
        &[OsStr::new("ls-files"), OsStr::new("-u"), OsStr::new("-z")],
    )?;
    let mut paths = BTreeSet::new();
    for field in raw.split(|b| *b == 0) {
        // Each record is "<mode> <sha> <stage>\t<path>".
        if let Some(tab) = field.iter().position(|b| *b == b'\t') {
            let path = path_from_bytes(&field[tab + 1..]);
            if !path.as_os_str().is_empty() {
                paths.insert(path);
            }
        }
    }
    Ok(paths)
}

/// Parse `git status --porcelain=v1 -z` output. Records are NUL-separated;
/// renames and copies carry a second NUL-terminated field holding the
/// original path.
fn parse_porcelain(data: &[u8]) -> Result<Vec<RawChange>, GitChaiError> {
    let fields: Vec<&[u8]> = data.split(|b| *b == 0).collect();
    let mut changes = Vec::new();
    let mut i = 0;

    while i < fields.len() {
        let field = fields[i];
        // "XY <path>"; shorter fields are the trailing NUL or malformed.
        if field.len() < 3 {
            i += 1;
            continue;
        }

        let status = GitStatus::parse(&field[..2])?;
        let filename = path_from_bytes(&field[3..]);

        let (old_filename, consumed) =
            if status.index == StatusCode::Renamed || status.index == StatusCode::Copied {
                if i + 1 < fields.len() {
                    (Some(path_from_bytes(fields[i + 1])), 2)
                } else {
                    (None, 1)
                }
            } else {
                (None, 1)
            };
        i += consumed;

        if filename.as_os_str().is_empty() {
            continue;
        }

        changes.push(RawChange {
            status,
            filename,
            old_filename,
        });
    }

    Ok(changes)
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

    #[test]
    fn parses_plain_records() {
        let changes = parse_porcelain(b" M file.txt\0M  staged.txt\0?? new.txt\0").unwrap();
        assert_eq!(changes.len(), 3);
        assert_eq!(changes[0].filename, PathBuf::from("file.txt"));
        assert_eq!(changes[1].filename, PathBuf::from("staged.txt"));
        assert_eq!(changes[2].filename, PathBuf::from("new.txt"));
        assert!(changes[2].old_filename.is_none());
        assert_eq!(
            ChangeType::from_status(&changes[0].status),
            Some(ChangeType::Modify)
        );
        assert_eq!(
            ChangeType::from_status(&changes[2].status),
            Some(ChangeType::Add)
        );
    }

    #[test]
    fn parses_rename_as_new_then_old() {
        let changes = parse_porcelain(b"R  a2.txt\0a.txt\0").unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].filename, PathBuf::from("a2.txt"));
        assert_eq!(changes[0].old_filename, Some(PathBuf::from("a.txt")));
        assert_eq!(
            ChangeType::from_status(&changes[0].status),
            Some(ChangeType::Rename)
        );
    }

    #[test]
    fn parses_copy() {
        let changes = parse_porcelain(b"C  copy.txt\0orig.txt\0").unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].filename, PathBuf::from("copy.txt"));
        assert_eq!(changes[0].old_filename, Some(PathBuf::from("orig.txt")));
        assert_eq!(
            ChangeType::from_status(&changes[0].status),
            Some(ChangeType::Copy)
        );
    }

    #[test]
    fn parses_untracked_directory_with_trailing_slash() {
        let changes = parse_porcelain(b"?? newpkg/\0").unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].filename, PathBuf::from("newpkg/"));
        assert_eq!(
            ChangeType::from_status(&changes[0].status),
            Some(ChangeType::Add)
        );
    }

    #[test]
    fn preserves_spaces_in_paths() {
        let changes = parse_porcelain(b" M dir/with space.txt\0").unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].filename, PathBuf::from("dir/with space.txt"));
    }

    #[test]
    fn keeps_unmerged_records_for_later_filtering() {
        // Classification happens in get_changed_files so the skip can be
        // logged; parsing must not silently drop these.
        let changes = parse_porcelain(b"UU conflict.txt\0").unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(
            ChangeType::from_status(&changes[0].status),
            None,
            "unmerged statuses must not produce a change type"
        );
    }

    #[test]
    fn rejects_unknown_status_codes() {
        assert!(parse_porcelain(b"X  file.txt\0").is_err());
        assert!(parse_porcelain(b" M file.txt\0X  other.txt\0").is_err());
    }

    #[test]
    fn tolerates_trailing_nul_and_garbage() {
        let changes = parse_porcelain(b" M a.txt\0\0XY\0").unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].filename, PathBuf::from("a.txt"));
    }
}
