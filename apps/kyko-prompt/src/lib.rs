//! The prompt kyko gives bash, zsh and fish: one definition, so it looks the same in all of them.
//!
//! Colours are the terminal's own 16 (so the kyko / uQ theme restyles it) and the icons are Nerd
//! Font glyphs. The shells' scripts call `kyko-prompt render` before each prompt; this crate only
//! builds the text. The prompt marks (OSC 133) are the scripts' business, not this program's.

use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

impl Shell {
    pub fn parse(name: &str) -> Option<Shell> {
        match name {
            "bash" => Some(Shell::Bash),
            "zsh" => Some(Shell::Zsh),
            "fish" => Some(Shell::Fish),
            _ => None,
        }
    }
}

/// What `git status` said.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Git {
    /// Branch name, or the short commit when detached.
    pub head: String,
    pub staged: bool,
    pub modified: bool,
    pub untracked: bool,
    pub ahead: u32,
    pub behind: u32,
}

#[derive(Clone, Debug)]
pub struct Input {
    pub shell: Shell,
    pub status: i32,
    pub duration_ms: u64,
    pub jobs: u32,
    pub cwd: String,
    pub home: String,
    pub venv: Option<String>,
    pub git: Option<Git>,
}

/// The folder as shown: `~` for the home folder, at most the last four parts.
pub fn short_dir(cwd: &str, home: &str) -> String {
    let rel = match (home.is_empty(), cwd.strip_prefix(home)) {
        (false, Some(r)) if r.is_empty() || r.starts_with('/') => format!("~{r}"),
        _ => cwd.to_string(),
    };
    let parts: Vec<&str> = rel.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() <= 4 {
        return rel;
    }
    format!("…/{}", parts[parts.len() - 4..].join("/"))
}

/// `3s`, `1m5s`, `2h3m`: for a command that took at least three seconds.
pub fn duration(ms: u64) -> Option<String> {
    let s = ms / 1000;
    match s {
        0..=2 => None,
        3..=59 => Some(format!("{s}s")),
        60..=3599 => Some(format!("{}m{}s", s / 60, s % 60)),
        _ => Some(format!("{}h{}m", s / 3600, s % 3600 / 60)),
    }
}

struct Out {
    shell: Shell,
    text: String,
}

impl Out {
    /// Plain text, escaped for the shell's prompt syntax.
    fn plain(&mut self, s: &str) {
        for c in s.chars() {
            match (self.shell, c) {
                (Shell::Bash, '\\' | '$' | '`') => {
                    self.text.push('\\');
                    self.text.push(c);
                }
                (Shell::Zsh, '%') => self.text.push_str("%%"),
                _ => self.text.push(c),
            }
        }
    }

    /// Colour codes, wrapped so the shell does not count them as width.
    fn sgr(&mut self, code: &str) {
        let seq = format!("\x1b[{code}m");
        match self.shell {
            Shell::Bash => {
                self.text.push('\x01');
                self.text.push_str(&seq);
                self.text.push('\x02');
            }
            Shell::Zsh => {
                self.text.push_str("%{");
                self.text.push_str(&seq);
                self.text.push_str("%}");
            }
            Shell::Fish => self.text.push_str(&seq),
        }
    }

    fn styled(&mut self, code: &str, s: &str) {
        self.sgr(code);
        self.plain(s);
        self.sgr("0");
    }
}

/// The prompt: a line of folder, git and tool facts, then the status and the `❯`.
pub fn render(i: &Input) -> String {
    let mut o = Out {
        shell: i.shell,
        text: String::new(),
    };
    o.styled("1;34", &format!(" {}", short_dir(&i.cwd, &i.home)));
    if let Some(g) = &i.git {
        o.plain(" ");
        o.styled("1;35", &format!(" {}", g.head));
        let flags: String = [(g.staged, "+"), (g.modified, "!"), (g.untracked, "?")]
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, f)| *f)
            .collect();
        let sync = format!(
            "{}{}",
            if g.ahead > 0 {
                format!("⇡{}", g.ahead)
            } else {
                String::new()
            },
            if g.behind > 0 {
                format!("⇣{}", g.behind)
            } else {
                String::new()
            }
        );
        if !flags.is_empty() || !sync.is_empty() {
            o.plain(" ");
            o.styled("33", &format!("[{flags}{sync}]"));
        }
    }
    if let Some(v) = &i.venv {
        o.plain(" ");
        o.styled("36", &format!(" {v}"));
    }
    if let Some(d) = duration(i.duration_ms) {
        o.plain(" ");
        o.styled("33", &format!(" {d}"));
    }
    if i.jobs > 0 {
        o.plain(" ");
        o.styled("36", &format!("✦{}", i.jobs));
    }
    o.plain("\n");
    if i.status != 0 {
        o.styled("1;31", &format!("✘ {} ", i.status));
    }
    o.styled(if i.status == 0 { "1;32" } else { "1;31" }, "❯");
    o.plain(" ");
    o.text
}

/// Parses `git status --porcelain=v2 --branch` output.
pub fn parse_git(text: &str) -> Option<Git> {
    let mut g = Git::default();
    let mut found = false;
    for line in text.lines() {
        if let Some(h) = line.strip_prefix("# branch.head ") {
            found = true;
            g.head = h.to_string();
        } else if let Some(o) = line.strip_prefix("# branch.oid ") {
            if g.head.is_empty() {
                g.head = o.chars().take(7).collect();
            }
        } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
            for w in ab.split_whitespace() {
                if let Some(n) = w.strip_prefix('+') {
                    g.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = w.strip_prefix('-') {
                    g.behind = n.parse().unwrap_or(0);
                }
            }
        } else if line.starts_with("1 ") || line.starts_with("2 ") || line.starts_with("u ") {
            let xy = line.split(' ').nth(1).unwrap_or("..");
            let mut c = xy.chars();
            g.staged |= c.next().is_some_and(|x| x != '.');
            g.modified |= c.next().is_some_and(|y| y != '.');
        } else if line.starts_with("? ") {
            g.untracked = true;
        }
    }
    if g.head == "(detached)" {
        g.head = "detached".into();
    }
    found.then_some(g)
}

/// The facts about the folder, gathered by running git (given at most 300 ms).
pub fn git_status(cwd: &Path) -> Option<Git> {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let mut child = Command::new("git")
        .args([
            "--no-optional-locks",
            "status",
            "--porcelain=v2",
            "--branch",
        ])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait().ok()? {
            Some(s) if s.success() => break,
            Some(_) => return None,
            None if start.elapsed() > Duration::from_millis(300) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(2)),
        }
    }
    let mut text = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut text).ok()?;
    parse_git(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(shell: Shell) -> Input {
        Input {
            shell,
            status: 0,
            duration_ms: 0,
            jobs: 0,
            cwd: "/home/u/src/kyko".into(),
            home: "/home/u".into(),
            venv: None,
            git: None,
        }
    }

    #[test]
    fn folders_shorten() {
        assert_eq!(short_dir("/home/u", "/home/u"), "~");
        assert_eq!(short_dir("/home/user2/x", "/home/u"), "/home/user2/x");
        assert_eq!(short_dir("/home/u/a/b/c/d/e", "/home/u"), "…/b/c/d/e");
        assert_eq!(short_dir("/", ""), "/");
    }

    #[test]
    fn durations() {
        assert_eq!(duration(2999), None);
        assert_eq!(duration(3000).as_deref(), Some("3s"));
        assert_eq!(duration(65_000).as_deref(), Some("1m5s"));
        assert_eq!(duration(7_380_000).as_deref(), Some("2h3m"));
    }

    #[test]
    fn git_output_parses() {
        let t =
            "# branch.oid abc\n# branch.head main\n# branch.ab +2 -1\n1 .M N... a b c d e f\n? x\n";
        let g = parse_git(t).unwrap();
        assert_eq!((g.head.as_str(), g.ahead, g.behind), ("main", 2, 1));
        assert!(g.modified && g.untracked && !g.staged);
        assert_eq!(parse_git(""), None);
    }

    #[test]
    fn a_failed_command_shows_status_and_red_arrow() {
        let mut i = input(Shell::Fish);
        i.status = 130;
        let t = render(&i);
        assert!(t.contains("✘ 130"));
        assert!(t.contains("\x1b[1;31m❯"));
        assert!(render(&input(Shell::Fish)).contains("\x1b[1;32m❯"));
    }

    #[test]
    fn colours_are_wrapped_per_shell() {
        assert!(render(&input(Shell::Bash)).contains("\x01\x1b[1;34m\x02"));
        assert!(render(&input(Shell::Zsh)).contains("%{\x1b[1;34m%}"));
        assert!(!render(&input(Shell::Fish)).contains('\x01'));
    }

    #[test]
    fn folder_names_cannot_run_code_in_bash_or_zsh() {
        let mut i = input(Shell::Bash);
        i.cwd = "/tmp/$(id)`x`\\w".into();
        i.home = String::new();
        let t = render(&i);
        assert!(t.contains("\\$(id)\\`x\\`\\\\w"), "{t:?}");
        let mut z = input(Shell::Zsh);
        z.cwd = "/tmp/50%".into();
        z.home = String::new();
        assert!(render(&z).contains("50%%"));
    }

    #[test]
    fn git_branch_and_flags() {
        let mut i = input(Shell::Fish);
        i.git = Some(Git {
            head: "main".into(),
            modified: true,
            ahead: 1,
            ..Git::default()
        });
        let t = render(&i);
        assert!(t.contains(" main") && t.contains("[!⇡1]"), "{t:?}");
    }
}
