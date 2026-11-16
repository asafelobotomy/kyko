//! `Run` requests from other programs: what a command may be, and how it starts in a new tab.

/// The longest command `Run` accepts, in bytes.
pub const MAX_COMMAND: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunError {
    Empty,
    TooLong,
    HasNul,
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::Empty => write!(f, "the command is empty"),
            RunError::TooLong => write!(f, "the command is longer than {MAX_COMMAND} bytes"),
            RunError::HasNul => write!(f, "the command contains a NUL byte"),
        }
    }
}

/// A command of 1 to 4096 bytes with no NUL.
pub fn validate(command: &str) -> Result<(), RunError> {
    if command.trim().is_empty() {
        Err(RunError::Empty)
    } else if command.len() > MAX_COMMAND {
        Err(RunError::TooLong)
    } else if command.contains('\0') {
        Err(RunError::HasNul)
    } else {
        Ok(())
    }
}

/// The arguments that make the user's shell run `command`.
pub fn shell_args(command: &str) -> Vec<String> {
    vec!["-c".to_string(), command.to_string()]
}

/// A working directory a caller sent: `None` for the empty string, else the path as given.
pub fn cwd(cwd: &str) -> Option<String> {
    (!cwd.is_empty()).then(|| cwd.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ordinary_commands() {
        assert_eq!(validate("cargo build"), Ok(()));
        assert_eq!(validate(&"a".repeat(MAX_COMMAND)), Ok(()));
    }

    #[test]
    fn refuses_empty_long_and_nul() {
        assert_eq!(validate(""), Err(RunError::Empty));
        assert_eq!(validate("  \t"), Err(RunError::Empty));
        assert_eq!(
            validate(&"a".repeat(MAX_COMMAND + 1)),
            Err(RunError::TooLong)
        );
        assert_eq!(validate("a\0b"), Err(RunError::HasNul));
    }

    #[test]
    fn the_shell_gets_the_command_whole() {
        assert_eq!(shell_args("echo 'a b'; ls"), ["-c", "echo 'a b'; ls"]);
    }

    #[test]
    fn an_empty_cwd_means_the_default() {
        assert_eq!(cwd(""), None);
        assert_eq!(cwd("/tmp"), Some("/tmp".into()));
    }
}
