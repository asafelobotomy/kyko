//! Shell integration: the same marks and prompt in every shell kyko starts.
//!
//! The scripts in `shell/` are compiled in and written under a folder of kyko's own. For an
//! interactive bash, zsh or fish, [`plan`] adds the arguments or variables that make the shell
//! load them after the user's own startup files. They emit OSC 7 (folder) and OSC 133 (prompt,
//! command and exit status marks) and draw kyko's prompt (`kyko-prompt render`, if installed) when
//! the user's startup files set none of their own. Any other shell, and any shell started with a command,
//! is left alone.

use std::io;
use std::path::Path;

const BASH: &str = include_str!("../shell/bash.sh");
const ZSHENV: &str = include_str!("../shell/zsh/.zshenv");
const ZSHRC: &str = include_str!("../shell/zsh/.zshrc");
const FISH: &str = include_str!("../shell/fish.fish");

/// What to add to a shell's start: arguments, then variables.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// The environment `plan` reads, so tests need not touch the process's own.
pub struct Host {
    pub zdotdir: Option<String>,
}

impl Host {
    pub fn current() -> Host {
        Host {
            zdotdir: std::env::var("ZDOTDIR").ok().filter(|d| !d.is_empty()),
        }
    }
}

/// Writes the scripts into `dir` (created if needed), replacing any older copy.
pub fn install(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir.join("zsh"))?;
    for (name, text) in [
        ("bash.sh", BASH),
        ("fish.fish", FISH),
        ("zsh/.zshenv", ZSHENV),
        ("zsh/.zshrc", ZSHRC),
    ] {
        let path = dir.join(name);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(text) {
            std::fs::write(path, text)?;
        }
    }
    Ok(())
}

/// The shell's name: `/usr/bin/zsh` is `zsh`.
pub fn shell_name(shell: &str) -> &str {
    let base = shell.rsplit('/').next().unwrap_or(shell);
    base.strip_prefix('-').unwrap_or(base)
}

/// What to add when starting `shell` with `args` from the scripts in `dir`. Empty for a shell
/// that is not bash, zsh or fish, and for one that has `args` (it runs a command).
pub fn plan(shell: &str, args: &[String], dir: &Path, host: &Host) -> Plan {
    if !args.is_empty() {
        return Plan::default();
    }
    let d = dir.to_string_lossy();
    let mut p = Plan::default();
    match shell_name(shell) {
        "bash" => p.args = vec!["--rcfile".into(), format!("{d}/bash.sh")],
        "zsh" => {
            p.env.push(("ZDOTDIR".into(), format!("{d}/zsh")));
            if let Some(z) = &host.zdotdir {
                p.env.push(("KYKO_USER_ZDOTDIR".into(), z.clone()));
            }
        }
        "fish" => {
            let quoted = format!("{d}/fish.fish")
                .replace('\\', "\\\\")
                .replace('\'', "\\'");
            p.args = vec!["--init-command".into(), format!("source '{quoted}'")];
        }
        _ => return p,
    }
    p.env.push(("KYKO_PROMPT".into(), "1".into()));
    p.env.push(("KYKO_EXTRAS".into(), "1".into()));
    p.env.push(("TERM_PROGRAM".into(), "kyko".into()));
    p.env
        .push(("TERM_PROGRAM_VERSION".into(), crate::VERSION.into()));
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> Host {
        Host { zdotdir: None }
    }

    fn get<'a>(p: &'a Plan, k: &str) -> Option<&'a str> {
        p.env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    #[test]
    fn bash_gets_an_rcfile() {
        let p = plan("/usr/bin/bash", &[], Path::new("/r"), &host());
        assert_eq!(p.args, ["--rcfile", "/r/bash.sh"]);
        assert_eq!(get(&p, "KYKO_PROMPT"), Some("1"));
    }

    #[test]
    fn zsh_keeps_the_users_zdotdir() {
        let h = Host {
            zdotdir: Some("/home/u/.zsh".into()),
        };
        let p = plan("/bin/zsh", &[], Path::new("/r"), &h);
        assert!(p.args.is_empty());
        assert_eq!(get(&p, "ZDOTDIR"), Some("/r/zsh"));
        assert_eq!(get(&p, "KYKO_USER_ZDOTDIR"), Some("/home/u/.zsh"));
    }

    #[test]
    fn fish_quotes_its_path() {
        let p = plan("fish", &[], Path::new("/r'x"), &host());
        assert_eq!(p.args, ["--init-command", "source '/r\\'x/fish.fish'"]);
    }

    #[test]
    fn commands_and_other_shells_are_left_alone() {
        let c = vec!["-c".to_string(), "ls".to_string()];
        assert_eq!(plan("bash", &c, Path::new("/r"), &host()), Plan::default());
        assert_eq!(plan("nu", &[], Path::new("/r"), &host()), Plan::default());
    }

    #[test]
    fn install_writes_every_script_once() {
        let t = tempfile::tempdir().unwrap();
        install(t.path()).unwrap();
        install(t.path()).unwrap();
        for f in ["bash.sh", "fish.fish", "zsh/.zshenv", "zsh/.zshrc"] {
            assert!(t.path().join(f).is_file(), "{f}");
        }
    }
}
