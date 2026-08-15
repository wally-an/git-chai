use std::fmt;

use crate::error::GitChaiError;

/// A single status letter from one side of a porcelain `XY` pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusCode {
    Unmodified,  // ' '
    Modified,    // M
    Added,       // A
    Deleted,     // D
    Renamed,     // R
    Copied,      // C
    Unmerged,    // U
    TypeChanged, // T
    Untracked,   // ?
    Ignored,     // !
}

impl StatusCode {
    pub fn from_char(c: char) -> Option<Self> {
        match c {
            ' ' => Some(Self::Unmodified),
            'M' => Some(Self::Modified),
            'A' => Some(Self::Added),
            'D' => Some(Self::Deleted),
            'R' => Some(Self::Renamed),
            'C' => Some(Self::Copied),
            'U' => Some(Self::Unmerged),
            'T' => Some(Self::TypeChanged),
            '?' => Some(Self::Untracked),
            '!' => Some(Self::Ignored),
            _ => None,
        }
    }

    pub fn as_char(self) -> char {
        match self {
            Self::Unmodified => ' ',
            Self::Modified => 'M',
            Self::Added => 'A',
            Self::Deleted => 'D',
            Self::Renamed => 'R',
            Self::Copied => 'C',
            Self::Unmerged => 'U',
            Self::TypeChanged => 'T',
            Self::Untracked => '?',
            Self::Ignored => '!',
        }
    }
}

/// A porcelain `XY` pair: index status plus worktree status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitStatus {
    pub index: StatusCode,
    pub worktree: StatusCode,
}

impl GitStatus {
    pub fn parse(xy: &[u8]) -> Result<Self, GitChaiError> {
        if xy.len() != 2 {
            return Err(GitChaiError::ParseError(format!(
                "expected 2 status bytes, got {}",
                xy.len()
            )));
        }
        let index = StatusCode::from_char(xy[0] as char).ok_or_else(|| {
            GitChaiError::ParseError(format!("unknown index status code {:?}", xy[0] as char))
        })?;
        let worktree = StatusCode::from_char(xy[1] as char).ok_or_else(|| {
            GitChaiError::ParseError(format!("unknown worktree status code {:?}", xy[1] as char))
        })?;
        Ok(Self { index, worktree })
    }
}

impl fmt::Display for GitStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.index.as_char(), self.worktree.as_char())
    }
}

/// What a change means for a commit message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    Add,
    Modify,
    Delete,
    Rename,
    Copy,
}

impl ChangeType {
    /// Derive a change type from a porcelain status pair, or `None` for paths
    /// that must not be committed (unmerged, ignored).
    pub fn from_status(status: &GitStatus) -> Option<Self> {
        use StatusCode::*;
        if status.index == Ignored || status.worktree == Ignored {
            return None;
        }
        if status.index == Unmerged || status.worktree == Unmerged {
            return None;
        }
        if status.index == Renamed {
            return Some(Self::Rename);
        }
        if status.index == Copied {
            return Some(Self::Copy);
        }
        if status.worktree == Untracked {
            return Some(Self::Add);
        }
        // The worktree side reflects the latest state; fall back to the index
        // side when the worktree is untouched.
        let effective = if status.worktree != Unmodified {
            status.worktree
        } else {
            status.index
        };
        match effective {
            Added => Some(Self::Add),
            Modified | TypeChanged => Some(Self::Modify),
            Deleted => Some(Self::Delete),
            _ => None,
        }
    }
}

impl fmt::Display for ChangeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Add => write!(f, "add"),
            Self::Modify => write!(f, "mod"),
            Self::Delete => write!(f, "del"),
            Self::Rename => write!(f, "rename"),
            Self::Copy => write!(f, "copy"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status_codes() {
        assert_eq!(StatusCode::from_char(' '), Some(StatusCode::Unmodified));
        assert_eq!(StatusCode::from_char('M'), Some(StatusCode::Modified));
        assert_eq!(StatusCode::from_char('A'), Some(StatusCode::Added));
        assert_eq!(StatusCode::from_char('D'), Some(StatusCode::Deleted));
        assert_eq!(StatusCode::from_char('R'), Some(StatusCode::Renamed));
        assert_eq!(StatusCode::from_char('C'), Some(StatusCode::Copied));
        assert_eq!(StatusCode::from_char('U'), Some(StatusCode::Unmerged));
        assert_eq!(StatusCode::from_char('T'), Some(StatusCode::TypeChanged));
        assert_eq!(StatusCode::from_char('?'), Some(StatusCode::Untracked));
        assert_eq!(StatusCode::from_char('!'), Some(StatusCode::Ignored));
        assert_eq!(StatusCode::from_char('X'), None);
    }

    #[test]
    fn parses_status_pairs() {
        let s = GitStatus::parse(b"M ").unwrap();
        assert_eq!(s.index, StatusCode::Modified);
        assert_eq!(s.worktree, StatusCode::Unmodified);

        let s = GitStatus::parse(b" M").unwrap();
        assert_eq!(s.index, StatusCode::Unmodified);
        assert_eq!(s.worktree, StatusCode::Modified);

        let s = GitStatus::parse(b"??").unwrap();
        assert_eq!(s.worktree, StatusCode::Untracked);

        assert!(GitStatus::parse(b"X ").is_err());
        assert!(GitStatus::parse(b"M").is_err());
    }

    #[test]
    fn displays_status_pairs() {
        let s = GitStatus {
            index: StatusCode::Modified,
            worktree: StatusCode::Unmodified,
        };
        assert_eq!(s.to_string(), "M ");
        let s = GitStatus {
            index: StatusCode::Unmodified,
            worktree: StatusCode::Untracked,
        };
        assert_eq!(s.to_string(), " ?");
    }

    #[test]
    fn derives_change_types() {
        use StatusCode::*;
        let st = |index, worktree| GitStatus { index, worktree };

        assert_eq!(
            ChangeType::from_status(&st(Unmodified, Untracked)),
            Some(ChangeType::Add)
        );
        assert_eq!(
            ChangeType::from_status(&st(Added, Unmodified)),
            Some(ChangeType::Add)
        );
        assert_eq!(
            ChangeType::from_status(&st(Modified, Unmodified)),
            Some(ChangeType::Modify)
        );
        assert_eq!(
            ChangeType::from_status(&st(Unmodified, Modified)),
            Some(ChangeType::Modify)
        );
        assert_eq!(
            ChangeType::from_status(&st(Modified, Modified)),
            Some(ChangeType::Modify)
        );
        assert_eq!(
            ChangeType::from_status(&st(Unmodified, Deleted)),
            Some(ChangeType::Delete)
        );
        assert_eq!(
            ChangeType::from_status(&st(Deleted, Unmodified)),
            Some(ChangeType::Delete)
        );
        assert_eq!(
            ChangeType::from_status(&st(Renamed, Unmodified)),
            Some(ChangeType::Rename)
        );
        assert_eq!(
            ChangeType::from_status(&st(Copied, Unmodified)),
            Some(ChangeType::Copy)
        );
        assert_eq!(
            ChangeType::from_status(&st(TypeChanged, Unmodified)),
            Some(ChangeType::Modify)
        );
        // Unmerged on either side never commits.
        assert_eq!(ChangeType::from_status(&st(Unmerged, Unmerged)), None);
        assert_eq!(ChangeType::from_status(&st(Added, Unmerged)), None);
        assert_eq!(ChangeType::from_status(&st(Unmerged, Added)), None);
        // Ignored never commits.
        assert_eq!(ChangeType::from_status(&st(Unmodified, Ignored)), None);
    }

    #[test]
    fn displays_change_types() {
        assert_eq!(ChangeType::Add.to_string(), "add");
        assert_eq!(ChangeType::Modify.to_string(), "mod");
        assert_eq!(ChangeType::Delete.to_string(), "del");
        assert_eq!(ChangeType::Rename.to_string(), "rename");
        assert_eq!(ChangeType::Copy.to_string(), "copy");
    }
}
