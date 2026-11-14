//! `Modes` against the emulator the window runs.
//!
//! For each random stream: feed it to an `Emulator`; separately, feed it to `Modes` (in random
//! chunks) and feed only `Modes::preamble()` to a fresh `Emulator`. The two emulators must be
//! in the same modes, and stay so through the same probes: kitty pops and screen switches,
//! which compare the kitty stacks entry by entry (the emulator does not expose them).
//!
//! `KYKO_DIFF_STREAMS` sets the number of streams (default 10,000) and `KYKO_DIFF_SEED` the
//! first seed. A failure prints its seed and a shortened stream.

use kyko_term::Emulator;
use kykod::modes::Modes;

/// Pieces streams are made of: every sequence `Modes` tracks, sequences it must ignore, and
/// ways a stream can be malformed.
const VOCABULARY: &[&[u8]] = &[
    // Private modes, tracked.
    b"\x1b[?1h",
    b"\x1b[?1l",
    b"\x1b[?6h",
    b"\x1b[?6l",
    b"\x1b[?7h",
    b"\x1b[?7l",
    b"\x1b[?12h",
    b"\x1b[?12l",
    b"\x1b[?25h",
    b"\x1b[?25l",
    b"\x1b[?1000h",
    b"\x1b[?1000l",
    b"\x1b[?1002h",
    b"\x1b[?1002l",
    b"\x1b[?1003h",
    b"\x1b[?1003l",
    b"\x1b[?1004h",
    b"\x1b[?1004l",
    b"\x1b[?1005h",
    b"\x1b[?1005l",
    b"\x1b[?1006h",
    b"\x1b[?1006l",
    b"\x1b[?1007h",
    b"\x1b[?1007l",
    b"\x1b[?1042h",
    b"\x1b[?1042l",
    b"\x1b[?1049h",
    b"\x1b[?1049l",
    b"\x1b[?2004h",
    b"\x1b[?2004l",
    b"\x1b[?1;1000;2004h",
    b"\x1b[?1006;25l",
    // Private modes this emulator ignores, or that change no tracked mode.
    b"\x1b[?47h",
    b"\x1b[?47l",
    b"\x1b[?1047h",
    b"\x1b[?1047l",
    b"\x1b[?1015h",
    b"\x1b[?9h",
    b"\x1b[?3h",
    b"\x1b[?3l",
    b"\x1b[?h",
    b"\x1b[?0h",
    // Synchronised output: the emulator holds output back until it ends.
    b"\x1b[?2026h",
    b"\x1b[?2026l",
    // ANSI modes.
    b"\x1b[4h",
    b"\x1b[4l",
    b"\x1b[20h",
    b"\x1b[20l",
    b"\x1b[4;20h",
    b"\x1b[2h",
    // Keypad, resets.
    b"\x1b=",
    b"\x1b>",
    b"\x1bc",
    b"\x1b[!p",
    // Cursor style.
    b"\x1b[0 q",
    b"\x1b[1 q",
    b"\x1b[2 q",
    b"\x1b[3 q",
    b"\x1b[4 q",
    b"\x1b[5 q",
    b"\x1b[6 q",
    b"\x1b[7 q",
    b"\x1b[ q",
    // Kitty keyboard.
    b"\x1b[>u",
    b"\x1b[>1u",
    b"\x1b[>3u",
    b"\x1b[>5u",
    b"\x1b[>31u",
    b"\x1b[>63u",
    b"\x1b[>257u",
    b"\x1b[<u",
    b"\x1b[<0u",
    b"\x1b[<2u",
    b"\x1b[=5u",
    b"\x1b[=1;1u",
    b"\x1b[=4;2u",
    b"\x1b[=1;3u",
    b"\x1b[=2;0u",
    b"\x1b[=u",
    b"\x1b[?u",
    // Subparameters, and parameter and intermediate overflow.
    b"\x1b[?2004:3h",
    b"\x1b[?1:2:3;25l",
    b"\x1b[?1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;2004h",
    b"\x1b[?1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;1;2004h",
    b"\x1b[?  1h",
    b"\x1b[?1 h",
    b"\x1b[1?h",
    // Controls inside a sequence; aborts.
    b"\x1b[?20\r04h",
    b"\x1b[?1\n049h",
    b"\x1b[?1049\x18h",
    b"\x1b[?10\x1a49h",
    b"\x1b\x1b[?1h",
    // Strings, terminated and not.
    b"\x1b]0;title\x07",
    b"\x1b]2;t\x1b\\",
    b"\x1b]0;open",
    b"\x1b]0;x\x9c",
    b"\x1bPq#0;1;2\x1b\\",
    b"\x1bP1$r",
    b"\x1bPq\x07",
    b"\x1bPq\x9c",
    b"\x1b_apc",
    b"\x1b^pm\x07",
    b"\x1bXsos\x1b\\",
    b"\x1b]0;\x1b[?1049h\x07",
    // Escapes with intermediates, other CSIs, title stack.
    b"\x1b(0",
    b"\x1b(B",
    b"\x1b#8",
    b"\x1b7",
    b"\x1b8",
    b"\x1b[2;5r",
    b"\x1b[r",
    b"\x1b[22t",
    b"\x1b[23t",
    b"\x1b[5;10H",
    b"\x1b[1;31m",
    b"\x1b[0m",
    b"\x1b[2J",
    b"\x1b[s",
    b"\x1b[u",
    // Text, UTF-8 (whole, cut, broken), controls.
    b"hello",
    b" ",
    b"\r\n",
    b"\n",
    b"\t",
    b"\x07",
    b"caf\xc3\xa9",
    b"\xf0\x9f\x98\x80",
    b"\xe2\x94",
    b"\x80",
    b"\xc3",
    b"\xff",
    b"\x9b?1049h",
    b"\x18",
    b"\x1a",
    b"\x1b",
];

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn emulator() -> Emulator {
    let mut e = Emulator::new(20, 5, 0);
    e.set_kitty_keyboard(true);
    e
}

fn feed(e: &mut Emulator, bytes: &[u8]) {
    e.feed(bytes);
    if e.pending_sync() {
        e.flush_sync();
    }
}

/// The probes fed to both emulators after the comparison, each compared after.
fn probes() -> Vec<&'static [u8]> {
    let pops = || std::iter::repeat_n(&b"\x1b[<u"[..], 6);
    pops()
        .chain([&b"\x1b[?1049l"[..]])
        .chain(pops())
        .chain([&b"\x1b[?1049h"[..]])
        .chain(pops())
        .chain([&b"\x1b[?1049l"[..]])
        .chain(pops())
        .collect()
}

/// Feeds `stream` to `Modes` in chunks cut by `cuts`.
fn modes_of(stream: &[u8], cuts: &[usize]) -> Modes {
    let mut m = Modes::new();
    let mut at = 0;
    for &c in cuts.iter().filter(|&&c| c <= stream.len()) {
        if c > at {
            m.feed(&stream[at..c]);
            at = c;
        }
    }
    m.feed(&stream[at..]);
    m
}

/// `None` if the two emulators agree throughout; otherwise what differed.
fn check(stream: &[u8], cuts: &[usize]) -> Option<String> {
    let mut live = emulator();
    feed(&mut live, stream);
    let preamble = modes_of(stream, cuts).preamble();
    let mut rebuilt = emulator();
    feed(&mut rebuilt, &preamble);
    if live.modes() != rebuilt.modes() {
        return Some(format!(
            "after the stream\n  live:    {:?}\n  rebuilt: {:?}\n  preamble: {:?}",
            live.modes(),
            rebuilt.modes(),
            String::from_utf8_lossy(&preamble)
        ));
    }
    for (i, probe) in probes().into_iter().enumerate() {
        feed(&mut live, probe);
        feed(&mut rebuilt, probe);
        if live.modes() != rebuilt.modes() {
            return Some(format!(
                "after probe {i} ({:?})\n  live:    {:?}\n  rebuilt: {:?}\n  preamble: {:?}",
                String::from_utf8_lossy(probe),
                live.modes(),
                rebuilt.modes(),
                String::from_utf8_lossy(&preamble)
            ));
        }
    }
    None
}

fn join(pieces: &[&[u8]]) -> Vec<u8> {
    pieces.concat()
}

/// Drops pieces one at a time while the stream still fails.
fn shorten(mut pieces: Vec<&[u8]>) -> Vec<&[u8]> {
    let mut i = 0;
    while i < pieces.len() {
        let mut fewer = pieces.clone();
        fewer.remove(i);
        if check(&join(&fewer), &[]).is_some() {
            pieces = fewer;
        } else {
            i += 1;
        }
    }
    pieces
}

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[test]
fn modes_rebuild_what_the_emulator_has() {
    let streams = env("KYKO_DIFF_STREAMS", 10_000);
    let first_seed = env("KYKO_DIFF_SEED", 1);
    for seed in first_seed..first_seed + streams {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let n = 1 + rng.below(60);
        let pieces: Vec<&[u8]> = (0..n)
            .map(|_| VOCABULARY[rng.below(VOCABULARY.len())])
            .collect();
        let stream = join(&pieces);
        let cuts: Vec<usize> = {
            let mut c: Vec<usize> = (0..rng.below(6))
                .map(|_| rng.below(stream.len() + 1))
                .collect();
            c.sort_unstable();
            c
        };
        if let Some(why) = check(&stream, &cuts) {
            let short = shorten(pieces);
            let shown: Vec<String> = short
                .iter()
                .map(|p| String::from_utf8_lossy(p).escape_debug().to_string())
                .collect();
            let why = check(&join(&short), &[]).unwrap_or(why);
            panic!("seed {seed}: {why}\n  shortened stream: {shown:?}");
        }
    }
}
