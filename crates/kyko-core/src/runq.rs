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

/// The shells a `#!` line may pick, by name.
const SHELLS: [&str; 6] = ["bash", "zsh", "fish", "sh", "dash", "ksh"];

/// The shell and arguments for a command: the user's `login_shell` with `-c`, unless the
/// command starts with a `#!` line naming another shell (`#!/usr/bin/env zsh`, `#!/bin/bash`),
/// which then runs the rest. Arguments that are not `-c <command>` are returned as they are.
pub fn resolve(login_shell: &str, args: Vec<String>) -> (String, Vec<String>) {
    let [flag, command] = args.as_slice() else {
        return (login_shell.to_string(), args);
    };
    let (first, rest) = command.split_once('\n').unwrap_or((command, ""));
    let Some(line) = first.strip_prefix("#!") else {
        return (login_shell.to_string(), args);
    };
    let mut words = line.split_whitespace();
    let mut name = words.next().and_then(|w| w.rsplit('/').next());
    if name == Some("env") {
        name = words.find(|w| !w.starts_with('-'));
    }
    match name {
        Some(n) if SHELLS.contains(&n) && flag == "-c" => {
            (n.to_string(), shell_args(rest.trim_start_matches('\n')))
        }
        _ => (login_shell.to_string(), args),
    }
}

/// A working directory a caller sent: `None` for the empty string, else the path as given.
pub fn cwd(cwd: &str) -> Option<String> {
    (!cwd.is_empty()).then(|| cwd.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shebang_picks_the_shell() {
        let go = |c: &str| resolve("/bin/fish", shell_args(c));
        assert_eq!(go("ls"), ("/bin/fish".into(), shell_args("ls")));
        assert_eq!(
            go("#!/bin/bash\necho hi"),
            ("bash".into(), shell_args("echo hi"))
        );
        assert_eq!(
            go("#!/usr/bin/env zsh\nls"),
            ("zsh".into(), shell_args("ls"))
        );
        assert_eq!(
            go("#!/usr/bin/python\nx"),
            ("/bin/fish".into(), shell_args("#!/usr/bin/python\nx"))
        );
        assert_eq!(
            resolve("/bin/fish", Vec::new()),
            ("/bin/fish".into(), Vec::new())
        );
    }

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
