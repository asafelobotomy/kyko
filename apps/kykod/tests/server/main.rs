#[path = "../common/mod.rs"]
mod common;

use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::process::Stdio;
use std::time::Duration;

use common::{Client, Holder, LIMIT, bin, contains, count, exists, gone_within, spec, wait_exit};
use kyko_proto::{MAX_FRAME, ToHolder, ToWindow, VERSION};

/// Collects `pane`'s output up to its `Exited`, and every frame for it in
/// `after` more. Returns the output, the exit status and how many `Exited`
/// came in all.
fn output_and_exits(c: &mut Client, pane: u32, after: Duration) -> (Vec<u8>, i32, usize) {
    let end = std::time::Instant::now() + LIMIT;
    let mut out = Vec::new();
    let status = loop {
        let left = end.saturating_duration_since(std::time::Instant::now());
        match c.recv(left) {
            Some(ToWindow::Output { pane: p, bytes }) if p == pane => out.extend(bytes),
            Some(ToWindow::Exited { pane: p, status }) if p == pane => break status,
            Some(_) => {}
            None => panic!(
                "no Exited for pane {pane}; output {:?}",
                String::from_utf8_lossy(&out)
            ),
        }
    };
    let mut exits = 1;
    let end = std::time::Instant::now() + after;
    while let Some(f) = c.recv(end.saturating_duration_since(std::time::Instant::now())) {
        if matches!(f, ToWindow::Exited { pane: p, .. } if p == pane) {
            exits += 1;
        }
    }
    (out, status, exits)
}

mod basics;
mod edge_cases;
mod limits;
