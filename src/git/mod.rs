mod commit;
mod grouping;
mod status;

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

pub use commit::{commit_paths, directory_commit_message, file_commit_message, push_changes};
pub use grouping::{ChangeGroup, group_changes_by_directory};
pub use status::get_changed_files;

/// Run `git <args>` in `repo`, optionally against an alternate index file,
/// and return stdout as raw bytes. Fails with the command line and stderr on
/// non-zero exit.
pub(crate) fn git_capture(
    repo: &Path,
    index_file: Option<&Path>,
    args: &[&OsStr],
) -> Result<Vec<u8>, crate::error::GitChaiError> {
    let mut cmd = Command::new("git");
    cmd.current_dir(repo);
    if let Some(index) = index_file {
        cmd.env("GIT_INDEX_FILE", index);
    }
    cmd.args(args);

    let command_str = format!(
        "git {}",
        args.iter()
            .map(|a| a.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    );

    let output = cmd.output().map_err(crate::error::GitChaiError::IoError)?;
    if !output.status.success() {
        return Err(crate::error::GitChaiError::GitCommandError {
            command: command_str,
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(output.stdout)
}

/// Wrap a path in the `:(literal)` pathspec magic so filenames containing
/// glob characters (`*`, `[`, `?`) are matched verbatim.
pub(crate) fn literal_pathspec(path: &Path) -> std::ffi::OsString {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let mut bytes = b":(literal)".to_vec();
        bytes.extend_from_slice(path.as_os_str().as_bytes());
        std::ffi::OsString::from_vec(bytes)
    }
    #[cfg(not(unix))]
    {
        std::ffi::OsString::from(format!(":(literal){}", path.display()))
    }
}
