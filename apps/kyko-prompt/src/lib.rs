//! The prompt: a folder pill, git state and context chips on the first line, status, duration and
//! time at its right edge, and `❯` on the second. Colours are the terminal theme's, passed in
//! `KYKO_PALETTE`; icons are Nerd Font, Unicode or ASCII (`KYKO_PROMPT_ICONS`).
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The shell, which decides how a colour sequence is marked as not taking room.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    Plain,
}

impl Shell {
    pub fn parse(s: &str) -> Shell {
        match s {
            "bash" => Shell::Bash,
            "zsh" => Shell::Zsh,
            "fish" => Shell::Fish,
            _ => Shell::Plain,
        }
    }

    /// `seq` (an escape sequence) as the shell must see it in a prompt.
    fn wrap(self, seq: &str) -> String {
        match self {
            Shell::Bash => format!("\\[{seq}\\]"),
            Shell::Zsh => format!("%{{{seq}%}}"),
            Shell::Fish | Shell::Plain => seq.to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icons {
    Nerd,
    Unicode,
    Ascii,
}

impl Icons {
    pub fn from_env() -> Icons {
        match std::env::var("KYKO_PROMPT_ICONS").as_deref() {
            Ok("nerd") => Icons::Nerd,
            Ok("ascii") => Icons::Ascii,
            Ok("unicode") => Icons::Unicode,
            // Nerd glyphs are known to work inside kyko, which ships a symbol font; elsewhere
            // they might not.
            _ if std::env::var_os("KYKO").is_some() => Icons::Nerd,
            _ => Icons::Unicode,
        }
    }

    fn folder(self) -> &'static str {
        match self {
            Icons::Nerd => "\u{f07b} ",
            Icons::Unicode => "▸ ",
            Icons::Ascii => "",
        }
    }
    fn branch(self) -> &'static str {
        match self {
            Icons::Nerd => "\u{e0a0} ",
            Icons::Unicode => "⎇ ",
            Icons::Ascii => "",
        }
    }
    fn ok(self) -> &'static str {
        match self {
            Icons::Ascii => "ok",
            _ => "✔",
        }
    }
    fn fail(self) -> &'static str {
        match self {
            Icons::Ascii => "x",
            _ => "✘",
        }
    }
    fn lock(self) -> &'static str {
        match self {
            Icons::Nerd => " \u{f023}",
            Icons::Unicode => " 🔒",
            Icons::Ascii => " [ro]",
        }
    }
    fn jobs(self) -> &'static str {
        match self {
            Icons::Nerd => "\u{f013} ",
            Icons::Unicode => "⚙ ",
            Icons::Ascii => "jobs:",
        }
    }
    fn timer(self) -> &'static str {
        match self {
            Icons::Nerd => "\u{f017} ",
            Icons::Unicode => "⏱ ",
            Icons::Ascii => "",
        }
    }
    fn corner_top(self) -> &'static str {
        if self == Icons::Ascii { ",-" } else { "╭─" }
    }
    fn corner_bottom(self) -> &'static str {
        if self == Icons::Ascii { "`-" } else { "╰─" }
    }
    fn arrow(self) -> &'static str {
        if self == Icons::Ascii { ">" } else { "❯" }
    }
}

type Rgb = (u8, u8, u8);

/// The colours the prompt uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Palette {
    pub fg: Rgb,
    pub bg: Rgb,
    pub accent: Rgb,
    pub ok: Rgb,
    pub err: Rgb,
    pub warn: Rgb,
    pub dim: Rgb,
    pub git: Rgb,
    /// Colours are off (`NO_COLOR`).
    pub plain: bool,
}

impl Default for Palette {
    fn default() -> Self {
        // gruvbox dark, kyko's fallback theme.
        Palette {
            fg: (0xeb, 0xdb, 0xb2),
            bg: (0x28, 0x28, 0x28),
            accent: (0xd7, 0x99, 0x21),
            ok: (0xb8, 0xbb, 0x26),
            err: (0xfb, 0x49, 0x34),
            warn: (0xfa, 0xbd, 0x2f),
            dim: (0x92, 0x83, 0x74),
            git: (0xd3, 0x86, 0x9b),
            plain: false,
        }
    }
}

fn hex(s: &str) -> Option<Rgb> {
    let s = s.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some(((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

impl Palette {
    /// `KYKO_PALETTE="fg=#ebdbb2,bg=#282828,accent=…"`; names it does not know, and colours that
    /// are not `#rrggbb`, are ignored, so a partial palette still works.
    pub fn parse(text: &str) -> Palette {
        let mut p = Palette::default();
        for pair in text.split(',') {
            let Some((k, v)) = pair.split_once('=') else {
                continue;
            };
            let Some(c) = hex(v.trim()) else { continue };
            match k.trim() {
                "fg" => p.fg = c,
                "bg" => p.bg = c,
                "accent" => p.accent = c,
                "ok" => p.ok = c,
                "err" => p.err = c,
                "warn" => p.warn = c,
                "dim" => p.dim = c,
                "git" => p.git = c,
                _ => {}
            }
        }
        p
    }

    pub fn from_env() -> Palette {
        let mut p = Palette::parse(&std::env::var("KYKO_PALETTE").unwrap_or_default());
        p.plain = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        p
    }
}

/// What the shell and the environment tell the prompt.
#[derive(Clone, Debug)]
pub struct Input {
    pub shell: Shell,
    pub status: i32,
    pub duration_ms: u64,
    pub jobs: u32,
    pub width: usize,
    pub transient: bool,
    pub cwd: PathBuf,
    pub home: Option<PathBuf>,
    pub user: String,
    pub host: String,
    /// Time of day, `HH:MM`.
    pub clock: String,
    pub ssh: bool,
    pub root: bool,
    pub venv: Option<String>,
    /// The folder's git state, if it is in a repository; filled by [`Input::from_env`].
    pub git: Option<Git>,
    pub writable: bool,
}

impl Input {
    pub fn from_env() -> Input {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let width = var("COLUMNS").and_then(|c| c.parse().ok()).unwrap_or(80);
        let git = git_state(&cwd, Duration::from_millis(150));
        let writable = std::fs::metadata(&cwd).is_ok_and(|m| !m.permissions().readonly());
        Input {
            shell: Shell::Plain,
            status: 0,
            duration_ms: 0,
            jobs: 0,
            width,
            transient: false,
            home: var("HOME").map(PathBuf::from),
            user: var("USER").unwrap_or_default(),
            host: var("HOSTNAME")
                .or_else(|| var("HOST"))
                .or_else(|| {
                    std::fs::read_to_string("/etc/hostname")
                        .ok()
                        .map(|h| h.trim().to_string())
                })
                .unwrap_or_default(),
            clock: clock_now(),
            ssh: var("SSH_CONNECTION").is_some() || var("SSH_TTY").is_some(),
            root: var("USER").as_deref() == Some("root"),
            venv: var("VIRTUAL_ENV").map(|v| {
                Path::new(&v)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or(v)
            }),
            git,
            writable,
            cwd,
        }
    }
}

fn clock_now() -> String {
    // Local time without a time-zone library: ask the C library through `date`'s own logic is
    // overkill, so use `TZ`-less UTC offset from /etc/localtime is also overkill; `date` is fine
    // and runs in a millisecond.
    Command::new("date")
        .arg("+%H:%M")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// The git state of a folder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Git {
    pub branch: String,
    pub ahead: u32,
    pub behind: u32,
    pub staged: u32,
    pub changed: u32,
    pub untracked: u32,
    pub conflicted: u32,
}

/// Whether `dir` or a parent holds a `.git`; the cheap check that spares most folders a process.
fn in_repo(dir: &Path) -> bool {
    dir.ancestors().any(|d| d.join(".git").exists())
}

/// `git status --porcelain=v2 --branch` for `dir`, or `None` outside a repository or when git
/// takes longer than `limit`.
pub fn git_state(dir: &Path, limit: Duration) -> Option<Git> {
    if !in_repo(dir) {
        return None;
    }
    let mut child = Command::new("git")
        .args(["--no-optional-locks", "-c", "core.fsmonitor=false"])
        .args(["status", "--porcelain=v2", "--branch"])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("GIT_OPTIONAL_LOCKS", "0")
        .spawn()
        .ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait().ok()? {
            Some(s) if s.success() => break,
            Some(_) => return None,
            None if start.elapsed() > limit => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(3)),
        }
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    Some(parse_status(&out))
}

/// Reads `git status --porcelain=v2 --branch` output.
pub fn parse_status(text: &str) -> Git {
    let mut g = Git::default();
    for line in text.lines() {
        if let Some(b) = line.strip_prefix("# branch.head ") {
            g.branch = if b == "(detached)" {
                "detached".into()
            } else {
                b.into()
            };
        } else if let Some(b) = line.strip_prefix("# branch.oid ") {
            if g.branch.is_empty() {
                g.branch = b.chars().take(7).collect();
            }
        } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
            for w in ab.split_whitespace() {
                if let Some(n) = w.strip_prefix('+') {
                    g.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = w.strip_prefix('-') {
                    g.behind = n.parse().unwrap_or(0);
                }
            }
        } else if let Some(rest) = line.strip_prefix("1 ").or_else(|| line.strip_prefix("2 ")) {
            let xy: Vec<char> = rest.chars().take(2).collect();
            if xy.first().is_some_and(|c| *c != '.') {
                g.staged += 1;
            }
            if xy.get(1).is_some_and(|c| *c != '.') {
                g.changed += 1;
            }
        } else if line.starts_with("u ") {
            g.conflicted += 1;
        } else if line.starts_with("? ") {
            g.untracked += 1;
        }
    }
    g
}

/// A folder for the prompt: home as `~`, and past four parts the first part, `…`, and the last
/// three.
pub fn short_dir(cwd: &Path, home: Option<&Path>) -> String {
    let (prefix, rest): (&str, PathBuf) = match home.and_then(|h| cwd.strip_prefix(h).ok()) {
        Some(r) => ("~", r.to_path_buf()),
        None => ("", cwd.to_path_buf()),
    };
    let parts: Vec<String> = rest
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    if parts.len() > 4 {
        format!("{prefix}/…/{}", parts[parts.len() - 3..].join("/"))
    } else if parts.is_empty() {
        if prefix.is_empty() { "/" } else { prefix }.to_string()
    } else if prefix.is_empty() {
        format!("/{}", parts.join("/"))
    } else {
        format!("~/{}", parts.join("/"))
    }
}

/// `3.2s`, `1m05s`, `2h03m`.
pub fn human(ms: u64) -> String {
    let s = ms / 1000;
    if s < 60 {
        format!("{}.{}s", s, (ms % 1000) / 100)
    } else if s < 3600 {
        format!("{}m{:02}s", s / 60, s % 60)
    } else {
        format!("{}h{:02}m", s / 3600, (s % 3600) / 60)
    }
}

/// A piece of the line: its text and the colours it is drawn in.
struct Chunk {
    text: String,
    fg: Option<Rgb>,
    bg: Option<Rgb>,
    bold: bool,
}

impl Chunk {
    fn new(text: impl Into<String>, fg: Rgb) -> Chunk {
        Chunk {
            text: text.into(),
            fg: Some(fg),
            bg: None,
            bold: false,
        }
    }
    fn width(&self) -> usize {
        self.text.chars().count()
    }
}

struct Painter<'a> {
    shell: Shell,
    palette: &'a Palette,
}

impl Painter<'_> {
    fn paint(&self, c: &Chunk) -> String {
        if self.palette.plain {
            return c.text.clone();
        }
        let mut on = String::new();
        if c.bold {
            on.push_str("\x1b[1m");
        }
        if let Some((r, g, b)) = c.fg {
            on.push_str(&format!("\x1b[38;2;{r};{g};{b}m"));
        }
        if let Some((r, g, b)) = c.bg {
            on.push_str(&format!("\x1b[48;2;{r};{g};{b}m"));
        }
        if on.is_empty() {
            return c.text.clone();
        }
        format!(
            "{}{}{}",
            self.shell.wrap(&on),
            c.text,
            self.shell.wrap("\x1b[0m")
        )
    }

    fn line(&self, chunks: &[Chunk]) -> String {
        chunks.iter().map(|c| self.paint(c)).collect()
    }
}

fn git_chunks(g: &Git, icons: Icons, p: &Palette) -> Vec<Chunk> {
    let mut v = vec![Chunk::new(format!("{}{}", icons.branch(), g.branch), p.git)];
    let mut state = String::new();
    for (n, mark) in [
        (g.conflicted, "="),
        (g.staged, "✚"),
        (g.changed, "●"),
        (g.untracked, "…"),
    ] {
        if n > 0 {
            state.push_str(&format!(" {mark}{n}"));
        }
    }
    if icons == Icons::Ascii {
        state = state.replace('✚', "+").replace('●', "~").replace('…', "?");
    }
    if g.ahead > 0 {
        state.push_str(&format!(
            " {}{}",
            if icons == Icons::Ascii { "^" } else { "⇡" },
            g.ahead
        ));
    }
    if g.behind > 0 {
        state.push_str(&format!(
            " {}{}",
            if icons == Icons::Ascii { "v" } else { "⇣" },
            g.behind
        ));
    }
    if !state.is_empty() {
        v.push(Chunk::new(state, p.warn));
    }
    v
}

/// The prompt text for a shell.
pub fn render(i: &Input, p: &Palette, icons: Icons) -> String {
    let paint = Painter {
        shell: i.shell,
        palette: p,
    };
    let arrow_colour = if i.status == 0 { p.accent } else { p.err };
    let arrow = |text: &str| {
        let mut c = Chunk::new(text, arrow_colour);
        c.bold = true;
        c
    };
    if i.transient {
        return paint.line(&[arrow(&format!("{} ", icons.arrow()))]);
    }

    // The left side.
    let mut left: Vec<Chunk> = vec![Chunk::new(icons.corner_top(), p.dim)];
    let dir = {
        let mut c = Chunk::new(
            format!(
                " {}{}{} ",
                icons.folder(),
                short_dir(&i.cwd, i.home.as_deref()),
                if i.writable { "" } else { icons.lock() }
            ),
            p.bg,
        );
        c.bg = Some(p.accent);
        c.bold = true;
        c
    };
    left.push(Chunk::new(" ", p.fg));
    left.push(dir);
    if i.root || i.ssh {
        let who = if i.ssh {
            format!("{}@{}", i.user, i.host)
        } else {
            i.user.clone()
        };
        left.push(Chunk::new(
            format!(" {who}"),
            if i.root { p.err } else { p.warn },
        ));
    }
    if let Some(v) = &i.venv {
        left.push(Chunk::new(format!(" ({v})"), p.dim));
    }
    if let Some(g) = &i.git {
        left.push(Chunk::new(" ", p.fg));
        left.extend(git_chunks(g, icons, p));
    }

    // The right side.
    let mut right: Vec<Chunk> = Vec::new();
    if i.jobs > 0 {
        right.push(Chunk::new(format!("{}{} ", icons.jobs(), i.jobs), p.warn));
    }
    if i.status == 0 {
        right.push(Chunk::new(format!("{} ", icons.ok()), p.ok));
    } else {
        right.push(Chunk::new(format!("{} {} ", icons.fail(), i.status), p.err));
    }
    if i.duration_ms >= 2000 {
        right.push(Chunk::new(
            format!("{}{} ", icons.timer(), human(i.duration_ms)),
            p.warn,
        ));
    }
    if !i.clock.is_empty() {
        right.push(Chunk::new(&i.clock, p.dim));
    }

    let lw: usize = left.iter().map(Chunk::width).sum();
    let rw: usize = right.iter().map(Chunk::width).sum();
    let mut out = paint.line(&left);
    if lw + rw + 2 <= i.width {
        out.push_str(&" ".repeat(i.width - lw - rw));
        out.push_str(&paint.line(&right));
    }
    out.push('\n');
    out.push_str(&paint.line(&[
        Chunk::new(icons.corner_bottom(), p.dim),
        arrow(&format!("{} ", icons.arrow())),
    ]));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> Input {
        Input {
            shell: Shell::Plain,
            status: 0,
            duration_ms: 0,
            jobs: 0,
            width: 60,
            transient: false,
            cwd: PathBuf::from("/home/u/code/kyko"),
            home: Some(PathBuf::from("/home/u")),
            user: "u".into(),
            host: "box".into(),
            clock: "14:07".into(),
            ssh: false,
            root: false,
            venv: None,
            git: None,
            writable: true,
        }
    }

    fn plain() -> Palette {
        Palette {
            plain: true,
            ..Palette::default()
        }
    }

    #[test]
    fn folders_are_shortened() {
        let h = Some(Path::new("/home/u"));
        assert_eq!(short_dir(Path::new("/home/u"), h), "~");
        assert_eq!(short_dir(Path::new("/home/u/code"), h), "~/code");
        assert_eq!(short_dir(Path::new("/home/u/a/b/c/d/e"), h), "~/…/c/d/e");
        assert_eq!(short_dir(Path::new("/"), h), "/");
        assert_eq!(short_dir(Path::new("/etc/x"), h), "/etc/x");
        assert_eq!(short_dir(Path::new("/a/b/c/d/e"), None), "/…/c/d/e");
    }

    #[test]
    fn durations_are_human() {
        assert_eq!(human(3200), "3.2s");
        assert_eq!(human(65_000), "1m05s");
        assert_eq!(human(7_380_000), "2h03m");
    }

    #[test]
    fn git_status_is_read() {
        let g = parse_status(
            "# branch.oid abc\n# branch.head main\n# branch.upstream o/main\n# branch.ab +2 -1\n\
             1 M. N... 100644 100644 100644 a b f\n1 .M N... 100644 100644 100644 a b g\n\
             1 MM N... 100644 100644 100644 a b h\n? new\nu UU N... 1 1 1 1 a b c x\n",
        );
        assert_eq!(
            g,
            Git {
                branch: "main".into(),
                ahead: 2,
                behind: 1,
                staged: 2,
                changed: 2,
                untracked: 1,
                conflicted: 1
            }
        );
        assert_eq!(
            parse_status("# branch.oid 1234567890\n# branch.head (detached)\n").branch,
            "detached"
        );
    }

    #[test]
    fn the_plain_prompt_has_two_lines_and_a_right_edge() {
        let out = render(&input(), &plain(), Icons::Unicode);
        let lines: Vec<&str> = out.split('\n').collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("~/code/kyko"), "{out}");
        assert!(lines[0].ends_with("✔ 14:07"), "{out}");
        assert_eq!(lines[0].chars().count(), 60, "{out}");
        assert_eq!(lines[1], "╰─❯ ");
    }

    #[test]
    fn a_failure_shows_its_code_and_a_long_command_its_time() {
        let mut i = input();
        i.status = 130;
        i.duration_ms = 12_300;
        i.jobs = 2;
        let out = render(&i, &plain(), Icons::Unicode);
        assert!(out.contains("✘ 130"), "{out}");
        assert!(out.contains("12.3s"), "{out}");
        assert!(out.contains("⚙ 2"), "{out}");
    }

    #[test]
    fn git_ssh_root_and_venv_chips_appear_when_they_apply() {
        let mut i = input();
        i.git = Some(Git {
            branch: "main".into(),
            staged: 1,
            ahead: 1,
            ..Git::default()
        });
        i.ssh = true;
        i.root = true;
        i.user = "root".into();
        i.venv = Some("env".into());
        i.writable = false;
        let out = render(&i, &plain(), Icons::Ascii);
        assert!(out.contains("main +1 ^1"), "{out}");
        assert!(out.contains("root@box"), "{out}");
        assert!(out.contains("(env)"), "{out}");
        assert!(out.contains("[ro]"), "{out}");
        assert!(out.starts_with(",-"), "{out}");
        assert!(out.contains("`->"), "{out}");
    }

    #[test]
    fn a_narrow_terminal_drops_the_right_side() {
        let mut i = input();
        i.width = 20;
        let out = render(&i, &plain(), Icons::Unicode);
        assert!(!out.contains("14:07"), "{out}");
    }

    #[test]
    fn the_transient_prompt_is_one_short_line() {
        let mut i = input();
        i.transient = true;
        assert_eq!(render(&i, &plain(), Icons::Unicode), "❯ ");
    }

    #[test]
    fn colours_are_wrapped_for_each_shell() {
        let p = Palette::default();
        let bash = render(&input(), &p, Icons::Unicode);
        assert!(!bash.contains("\\["), "plain output has no wrappers");
        let mut i = input();
        i.shell = Shell::Bash;
        let out = render(&i, &p, Icons::Unicode);
        assert!(out.contains("\x1b[48;2;215;153;33m\\] ▸"), "{out:?}");
        assert!(out.contains("\\[\x1b[0m\\]"), "{out:?}");
        i.shell = Shell::Zsh;
        assert!(render(&i, &p, Icons::Unicode).contains("%{\x1b[0m%}"));
    }

    #[test]
    fn the_palette_is_read_and_a_bad_entry_is_ignored() {
        let p = Palette::parse("accent=#112233, bg=nope, junk, fg=#fff, err=#ff0000");
        assert_eq!(p.accent, (0x11, 0x22, 0x33));
        assert_eq!(p.err, (255, 0, 0));
        assert_eq!(p.bg, Palette::default().bg);
        assert_eq!(p.fg, Palette::default().fg);
    }

    #[test]
    fn git_is_read_from_a_real_repository() {
        let t = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(t.path())
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
        };
        git(&["init", "-q", "-b", "trunk"]);
        std::fs::write(t.path().join("a"), "x").unwrap();
        let g = git_state(t.path(), Duration::from_secs(5)).unwrap();
        assert_eq!(g.branch, "trunk");
        assert_eq!(g.untracked, 1);
        assert!(git_state(Path::new("/proc"), Duration::from_secs(5)).is_none());
    }
}
