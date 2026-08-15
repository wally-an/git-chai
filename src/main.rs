mod error;
mod git;
mod types;

use anyhow::Result;
use clap::Parser;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::git::{
    ChangeGroup, commit_paths, directory_commit_message, file_commit_message, get_changed_files,
    group_changes_by_directory, push_changes,
};

#[derive(Parser, Debug)]
#[command(about, long_about = None, disable_version_flag = true)]
struct Args {
    /// Path to git repository
    #[arg(short, long, default_value = ".")]
    repo_path: PathBuf,

    /// Push changes to remote after committing
    #[arg(short, long, default_value_t = false)]
    push: bool,

    /// Dry run - show what would be committed without actually committing
    #[arg(short, long, default_value_t = false)]
    dry_run: bool,

    /// Verbose output
    #[arg(short, long, default_value_t = false)]
    verbose: bool,

    /// Headless mode - run continuously until interrupted
    #[arg(short = '!', long, default_value_t = false)]
    headless: bool,

    /// Show version information
    #[arg(short = '?', long = "version")]
    version: bool,
}

fn process_changes(repo_path: &Path, dry_run: bool, push: bool) -> Result<()> {
    log::info!("Scanning for changes in {:?}...", repo_path);

    let changes = get_changed_files(repo_path)?;
    if changes.is_empty() {
        log::info!("No changes detected");
        return Ok(());
    }

    let groups = group_changes_by_directory(repo_path, changes)?;

    let mut committed = 0usize;
    let mut failed = 0usize;
    let mut planned = 0usize;

    for group in &groups {
        match group {
            ChangeGroup::Directory {
                path,
                change_type,
                files,
            } => {
                let message = directory_commit_message(path, *change_type);
                planned += 1;
                if dry_run {
                    log::info!(
                        "DRY RUN: would commit {} file(s) in {} as '{}'",
                        files.len(),
                        path.display(),
                        message
                    );
                    if log::log_enabled!(log::Level::Debug) {
                        for file in files {
                            log::debug!("DRY RUN:   {}", file.filename.display());
                        }
                    }
                    continue;
                }
                // Stage the exact changed files rather than the whole
                // directory: files created after the scan must not be
                // swept into this commit. For untracked directories the
                // entry is the directory itself ("newpkg/").
                let paths: Vec<PathBuf> = files.iter().map(|f| f.filename.clone()).collect();
                match commit_paths(repo_path, &paths, &message) {
                    Ok(()) => {
                        committed += 1;
                        log::info!("Committed {}: {}", change_type, path.display());
                        if log::log_enabled!(log::Level::Debug) {
                            for file in files {
                                log::debug!("  {}", file.filename.display());
                            }
                        }
                    }
                    Err(e) => {
                        failed += 1;
                        log::error!("Failed to commit {}: {}", path.display(), e);
                    }
                }
            }
            ChangeGroup::Individual { files } => {
                for file in files {
                    let message = file_commit_message(file);
                    planned += 1;
                    if dry_run {
                        log::info!("DRY RUN: would commit '{}'", message);
                        continue;
                    }
                    let mut paths = Vec::with_capacity(2);
                    if let Some(old) = &file.old_filename {
                        paths.push(old.clone());
                    }
                    paths.push(file.filename.clone());
                    match commit_paths(repo_path, &paths, &message) {
                        Ok(()) => {
                            committed += 1;
                            log::info!("Committed {}", message);
                        }
                        Err(e) => {
                            failed += 1;
                            log::error!("Failed to commit {}: {}", file.filename.display(), e);
                        }
                    }
                }
            }
        }
    }

    if dry_run {
        log::info!(
            "DRY RUN: no changes were committed ({} change(s) would have been committed)",
            planned
        );
        if push {
            log::info!("DRY RUN: would push changes to remote");
        }
        return Ok(());
    }

    if failed > 0 {
        let message = format!("{} of {} change(s) failed to commit", failed, planned);
        log::error!("{}", message);
        return Err(anyhow::anyhow!(message));
    }

    if committed == 0 {
        log::info!("No changes to commit");
        return Ok(());
    }

    log::info!("Successfully committed {} change(s)", committed);

    if push {
        match push_changes(repo_path) {
            Ok(()) => log::info!("Successfully pushed changes to remote!"),
            Err(e) => {
                log::warn!("Failed to push changes: {}", e);
                log::warn!("Changes were committed locally but not pushed to remote.");
            }
        }
    } else {
        log::info!("Skipping push");
    }

    Ok(())
}

fn resolve_repo_toplevel(path: &Path) -> Result<PathBuf> {
    let output = Command::new("git")
        .current_dir(path)
        .arg("rev-parse")
        .arg("--show-toplevel")
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run git rev-parse: {}", e))?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "git rev-parse --show-toplevel failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let toplevel = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(PathBuf::from(toplevel))
}

fn init_logging(verbose: bool) {
    // An empty RUST_LOG (e.g. exported by build sandboxes) would silently
    // mute all output; treat it as unset and use the default filter.
    let default = if verbose { "debug" } else { "info" };
    let filter = std::env::var("RUST_LOG")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_string());
    env_logger::Builder::new().parse_filters(&filter).init();
}

fn main() -> Result<()> {
    let args = Args::parse();

    init_logging(args.verbose);

    if args.version {
        println!("git-chai {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let repo_root = resolve_repo_toplevel(&args.repo_path).map_err(|e| {
        anyhow::anyhow!(
            "Failed to resolve git repo top-level for {:?}: {}",
            args.repo_path,
            e
        )
    })?;

    if args.headless {
        use std::thread;
        use std::time::Duration;

        let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let r = running.clone();
        ctrlc::set_handler(move || {
            r.store(false, std::sync::atomic::Ordering::SeqCst);
            println!("\nReceived interrupt signal, shutting down...");
        })
        .expect("Error setting Ctrl+C handler");

        log::info!("git-chai: Starting in headless mode. Press Ctrl+C to stop.");

        while running.load(std::sync::atomic::Ordering::SeqCst) {
            if let Err(e) = process_changes(&repo_root, args.dry_run, args.push) {
                log::error!("Error processing changes: {}", e);
            }

            log::info!("Waiting 5 seconds before next scan...");
            for _ in 0..50 {
                if !running.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
        }

        log::info!("git-chai stopped");
        Ok(())
    } else {
        log::info!("git-chai: Running once");
        process_changes(&repo_root, args.dry_run, args.push)
    }
}
