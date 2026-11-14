//! How fast `Modes` reads output, which the ring feeds it as it drops bytes (slice 1c). Not run
//! by default:
//!
//! ```sh
//! cargo test -p kykod --release --test modes_bench -- --ignored --nocapture
//! ```
//!
//! Target: at least 3.5 GB/s on `seq`-like output, so the hop bench (`hop_bench.rs`, 16.9 MB)
//! pays under 5 ms for it.

use std::time::Instant;

use kykod::modes::Modes;

const SIZE: usize = 64 << 20;
/// What the ring hands over at a time: a pane's read is at most 64 KiB.
const CHUNK: usize = 64 << 10;

fn seq_like() -> Vec<u8> {
    let mut out = Vec::with_capacity(SIZE + 16);
    let mut i = 1u64;
    while out.len() < SIZE {
        out.extend_from_slice(i.to_string().as_bytes());
        out.extend_from_slice(b"\r\n");
        i += 1;
    }
    out
}

/// Something like `ls -l --color`: an SGR pair every line.
fn coloured() -> Vec<u8> {
    let mut out = Vec::with_capacity(SIZE + 128);
    let mut i = 0u64;
    while out.len() < SIZE {
        out.extend_from_slice(
            format!(
                "-rw-r--r-- 1 user user {:>8} Nov 14 10:{:02} \x1b[0m\x1b[01;3{}mfile-{i}.txt\x1b[0m\r\n",
                i * 37 % 100_000,
                i % 60,
                i % 8
            )
            .as_bytes(),
        );
        i += 1;
    }
    out
}

fn measure(name: &str, data: &[u8]) -> f64 {
    let mut best = f64::MAX;
    for _ in 0..5 {
        let mut m = Modes::new();
        let started = Instant::now();
        for chunk in data.chunks(CHUNK) {
            m.feed(chunk);
        }
        best = best.min(started.elapsed().as_secs_f64());
        std::hint::black_box(&m);
    }
    let gbps = data.len() as f64 / best / 1e9;
    println!(
        "{name}: {:.1} MB in {:.2} ms (best of 5), {gbps:.2} GB/s",
        data.len() as f64 / 1e6,
        best * 1e3
    );
    gbps
}

#[test]
#[ignore]
fn modes_throughput() {
    let seq = measure("seq-like", &seq_like());
    measure("coloured", &coloured());
    assert!(seq >= 3.5, "seq-like output at {seq:.2} GB/s, under 3.5");
}
