//! The bash script, run for real: an interactive bash prints the prompt, command and status marks.

use std::io::Write;
use std::process::{Command, Stdio};

use kyko_core::integration::{Host, install, plan};

fn run(input: &str, bashrc: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let scripts = dir.path().join("shell");
    install(&scripts).unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir(&home).unwrap();
    std::fs::write(home.join(".bashrc"), bashrc).unwrap();
    // A stand-in for `kyko-prompt`, which shows what the script passed it.
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let stub = bin.join("kyko-prompt");
    std::fs::write(&stub, "#!/bin/sh\nprintf 'KP[%s]' \"$*\"\n").unwrap();
    std::fs::set_permissions(&stub, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let p = plan("bash", &[], &scripts, &Host { zdotdir: None });
    let mut child = Command::new("bash")
        .args(&p.args)
        .arg("-i")
        .envs(p.env.iter().map(|(k, v)| (k, v)))
        .env("HOME", &home)
        .env(
            "PATH",
            format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
        )
        .current_dir(&home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn bash_marks_prompt_command_and_status() {
    let out = run("true\nfalse\nexit\n", "");
    assert!(out.contains("\x1b]133;A\x07"), "prompt mark: {out:?}");
    assert!(
        out.contains("\x1b]133;C;cmdline_url=false\x07"),
        "command mark with its line: {out:?}"
    );
    // Arch's bash.bashrc adds to PROMPT_COMMAND; only the user's commands are marked.
    assert_eq!(out.matches("\x1b]133;C").count(), 3, "{out:?}");
    assert!(out.contains("\x1b]133;D;0\x07"), "success status: {out:?}");
    assert!(out.contains("\x1b]133;D;1\x07"), "failure status: {out:?}");
    assert!(out.contains("\x1b]7;file://"), "folder report: {out:?}");
}

#[test]
fn bash_draws_kyko_prompt_with_status_when_the_user_set_none() {
    let out = run("false\nexit\n", "");
    assert!(
        out.contains("KP[render --shell bash --status 1 --duration "),
        "{out:?}"
    );
    assert!(out.contains("\x1b]133;B\x07"), "input mark: {out:?}");
}

#[test]
fn bash_keeps_a_prompt_the_user_set() {
    let out = run("true\nexit\n", "PS1='mine> '\n");
    assert!(out.contains("mine> "), "{out:?}");
    assert!(!out.contains("KP["), "{out:?}");
    assert!(
        out.contains("\x1b]133;A\x07"),
        "marks still emitted: {out:?}"
    );
}

#[test]
fn bash_does_not_mark_the_users_own_prompt_commands() {
    // Prompt commands added as an array (bash 5.1), as Arch's bash.bashrc and zoxide do.
    let out = run("true\nexit\n", "PROMPT_COMMAND+=('printf \"\"' 'true')\n");
    assert_eq!(out.matches("\x1b]133;C").count(), 2, "{out:?}");
    assert_eq!(out.matches("\x1b]133;D").count(), 1, "{out:?}");
}
