//! The bash script, run for real: an interactive bash prints the prompt, command and status marks.

use std::io::Write;
use std::process::{Command, Stdio};

use kyko_core::integration::{Host, install, plan};

fn run(input: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let scripts = dir.path().join("shell");
    install(&scripts).unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let p = plan(
        "bash",
        &[],
        &scripts,
        &Host {
            starship_config_env: true,
            starship_config_file: true,
            zdotdir: None,
        },
    );
    let mut child = Command::new("bash")
        .args(&p.args)
        .arg("-i")
        .envs(p.env.iter().map(|(k, v)| (k, v)))
        .env("HOME", &home)
        .env("PATH", std::env::var("PATH").unwrap())
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
    let out = run("true\nfalse\nexit\n");
    assert!(out.contains("\x1b]133;A\x07"), "prompt mark: {out:?}");
    assert!(out.contains("\x1b]133;C\x07"), "command mark: {out:?}");
    assert!(out.contains("\x1b]133;D;0\x07"), "success status: {out:?}");
    assert!(out.contains("\x1b]133;D;1\x07"), "failure status: {out:?}");
    assert!(out.contains("\x1b]7;file://"), "folder report: {out:?}");
}
