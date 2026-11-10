//! The cost of the hop through `kykod` (spec §6.2 item 6). Run it in release
//! mode on an otherwise idle machine:
//! `cargo test -p kykod --release --test hop_bench -- --ignored --nocapture`
mod common;

use std::io::Read;
use std::time::{Duration, Instant};

use common::{Client, Holder, spec};
use kyko_proto::{ToHolder, ToWindow};
use kykod::pty::Pty;

const RUNS: usize = 5;
const ARGS: [&str; 2] = ["1", "2000000"];
/// The bytes `seq 1 2000000` writes to a terminal, each `\n` sent as `\r\n`.
const EXPECTED: usize = 16_888_896;

/// `seq` on a terminal read directly; returns the bytes read.
fn direct() -> usize {
    let args: Vec<String> = ARGS.iter().map(|a| a.to_string()).collect();
    let pty = Pty::spawn("/usr/bin/seq", &args, "", &[], 80, 24).unwrap();
    let mut reader = pty.reader().unwrap();
    let mut buf = vec![0u8; 64 * 1024];
    let mut total = 0;
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            // EIO: the program has gone and its output has all been read.
            Err(_) => break,
        }
    }
    assert_eq!(pty.wait(), Some(0));
    total
}

/// `seq` through `kykod`, as a window receives it; returns the bytes received.
fn through_holder(c: &mut Client, pane: u32) -> usize {
    c.send(ToHolder::Spawn(spec(pane, "/usr/bin/seq", &ARGS)));
    let mut total = 0;
    loop {
        match c.recv(Duration::from_secs(60)) {
            Some(ToWindow::Output { pane: p, bytes }) if p == pane => total += bytes.len(),
            Some(ToWindow::Exited { pane: p, status }) if p == pane => {
                assert_eq!(status, 0);
                return total;
            }
            Some(_) => {}
            None => panic!("pane {pane} did not finish; {total} bytes so far"),
        }
    }
}

fn median(mut times: Vec<Duration>) -> Duration {
    times.sort();
    times[times.len() / 2]
}

#[test]
#[ignore = "a timing check: run it alone, in release mode, on an idle machine"]
fn holder_hop_is_within_15_percent() {
    let h = Holder::start();
    let mut c = Client::connect(&h);
    c.hello();
    let mut direct_times = Vec::new();
    let mut holder_times = Vec::new();
    // Alternate the two, so a change in the machine's load hits both alike.
    for run in 0..RUNS {
        let start = Instant::now();
        assert_eq!(direct(), EXPECTED);
        direct_times.push(start.elapsed());

        let start = Instant::now();
        assert_eq!(through_holder(&mut c, run as u32 + 1), EXPECTED);
        holder_times.push(start.elapsed());
        c.send(ToHolder::Close {
            pane: run as u32 + 1,
        });
    }
    let (d, k) = (median(direct_times.clone()), median(holder_times.clone()));
    let ratio = k.as_secs_f64() / d.as_secs_f64();
    println!("direct runs: {direct_times:?}");
    println!("holder runs: {holder_times:?}");
    println!("direct median {d:?}, through kykod {k:?}, ratio {ratio:.3}");
    assert!(ratio <= 1.15, "the hop costs {ratio:.3}x");
}
