use thiserror::Error;

#[derive(Debug, Error)]
#[allow(clippy::enum_variant_names)] // Error suffix on every variant is deliberate
pub enum GitChaiError {
    #[error("Git command failed: {command}: {stderr}")]
    GitCommandError { command: String, stderr: String },

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    ParseError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_errors() {
        let git_error = GitChaiError::GitCommandError {
            command: "git status".to_string(),
            stderr: "fatal: not a git repository".to_string(),
        };
        assert_eq!(
            git_error.to_string(),
            "Git command failed: git status: fatal: not a git repository"
        );

        let io_error = GitChaiError::IoError(std::io::Error::other("test"));
        assert!(io_error.to_string().contains("IO error"));

        let parse_error = GitChaiError::ParseError("bad status".to_string());
        assert_eq!(parse_error.to_string(), "Parse error: bad status");
    }
}
