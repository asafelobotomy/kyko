//! `kyko-prompt render --shell bash|zsh|fish|plain [--status N] [--duration-ms N] [--jobs N]
//! [--width N] [--transient]`: prints the prompt for a shell's hook to use. See
//! `docs/superpowers/specs/2026-11-16-kyko-shell-design.md` §3.
use kyko_prompt::{Icons, Input, Palette, Shell, render};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("render") {
        eprintln!(
            "usage: kyko-prompt render [--shell bash|zsh|fish|plain] [--status N] [--duration-ms N]\n\
             \x20                         [--jobs N] [--width N] [--transient]"
        );
        std::process::exit(2);
    }
    let mut input = Input::from_env();
    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        fn num<T: std::str::FromStr>(it: &mut std::slice::Iter<String>) -> Option<T> {
            it.next().and_then(|v| v.parse().ok())
        }
        match a.as_str() {
            "--shell" => {
                input.shell = it.next().map(|s| Shell::parse(s)).unwrap_or(Shell::Plain);
            }
            "--status" => input.status = num(&mut it).unwrap_or(0),
            "--duration-ms" => input.duration_ms = num(&mut it).unwrap_or(0),
            "--jobs" => input.jobs = num(&mut it).unwrap_or(0),
            "--width" => input.width = num(&mut it).unwrap_or(80),
            "--transient" => input.transient = true,
            other => {
                eprintln!("kyko-prompt: unknown option {other}");
                std::process::exit(2);
            }
        }
    }
    let palette = Palette::from_env();
    let icons = Icons::from_env();
    print!("{}", render(&input, &palette, icons));
}
