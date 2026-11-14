//! A byte ring that keeps the newest output of a pane, and the terminal modes at its first
//! kept byte.

use std::collections::VecDeque;

use crate::modes::Modes;

const MIN_CAPACITY: usize = 256 * 1024;
const MAX_CAPACITY: usize = 16 * 1024 * 1024;

/// The most recent bytes a pane produced, up to a fixed capacity.
pub struct Ring {
    buf: VecDeque<u8>,
    capacity: usize,
    wrapped: bool,
    /// Has read every byte dropped so far, so it describes the terminal at the first kept byte.
    modes: Modes,
}

impl Ring {
    /// A ring holding up to `capacity` bytes, clamped to 256 KiB ..= 16 MiB.
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.clamp(MIN_CAPACITY, MAX_CAPACITY);
        Ring {
            buf: VecDeque::new(),
            capacity,
            wrapped: false,
            modes: Modes::new(),
        }
    }

    /// Appends `bytes`, dropping the oldest bytes beyond the capacity. Every dropped byte goes
    /// to the modes, oldest first.
    pub fn push(&mut self, bytes: &[u8]) {
        let bytes = if bytes.len() > self.capacity {
            // All the old bytes go, then the head of these.
            self.drop_oldest(self.buf.len());
            let cut = bytes.len() - self.capacity;
            self.modes.feed(&bytes[..cut]);
            self.wrapped = true;
            &bytes[cut..]
        } else {
            bytes
        };
        let overflow = (self.buf.len() + bytes.len()).saturating_sub(self.capacity);
        if overflow > 0 {
            self.drop_oldest(overflow);
            self.wrapped = true;
        }
        self.buf.extend(bytes);
    }

    /// Drops the `n` oldest bytes, feeding them to the modes as slices.
    fn drop_oldest(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        let (a, b) = self.buf.as_slices();
        let from_a = n.min(a.len());
        self.modes.feed(&a[..from_a]);
        self.modes.feed(&b[..n - from_a]);
        self.buf.drain(..n);
    }

    /// What a window needs to rebuild the pane, and whether anything was dropped. Untruncated:
    /// every byte. Truncated: the bytes that restore the terminal modes in force where the
    /// replay starts, then the kept bytes from there. That place is a clean one
    /// ([`Modes::find_start`]), so no partial escape sequence or character reaches the window.
    pub fn replay(&self) -> (Vec<u8>, bool) {
        if !self.wrapped {
            return (self.contents(), false);
        }
        let kept = self.contents();
        let mut modes = self.modes.clone();
        let start = modes.find_start(&kept);
        let mut out = modes.preamble();
        out.extend_from_slice(&kept[start..]);
        (out, true)
    }

    /// Everything currently held, oldest first.
    pub fn contents(&self) -> Vec<u8> {
        let (a, b) = self.buf.as_slices();
        let mut out = Vec::with_capacity(self.buf.len());
        out.extend_from_slice(a);
        out.extend_from_slice(b);
        out
    }

    /// True once any byte has been dropped.
    pub fn truncated(&self) -> bool {
        self.wrapped
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_everything_under_capacity() {
        let mut r = Ring::new(262_144);
        r.push(b"abc");
        assert_eq!(r.contents(), b"abc");
        assert!(!r.truncated());
    }

    #[test]
    fn drops_the_oldest_beyond_capacity() {
        let mut r = Ring::new(262_144);
        r.push(&vec![b'a'; 262_144]);
        assert!(!r.truncated());
        r.push(b"xyz");
        let c = r.contents();
        assert_eq!(c.len(), 262_144);
        assert!(c.ends_with(b"xyz"));
        assert!(c.starts_with(b"aaa"));
        assert!(r.truncated());
    }

    #[test]
    fn one_push_larger_than_capacity_keeps_its_tail() {
        let mut r = Ring::new(262_144);
        let mut big = vec![b'a'; 300_000];
        big.extend(b"END");
        r.push(&big);
        assert_eq!(r.len(), 262_144);
        assert!(r.contents().ends_with(b"END"));
        assert!(r.truncated());
    }

    #[test]
    fn capacity_is_clamped() {
        assert_eq!(Ring::new(1).capacity(), 262_144);
        assert_eq!(Ring::new(usize::MAX).capacity(), 16 * 1024 * 1024);
    }

    #[test]
    fn untruncated_replay_is_every_byte() {
        let mut r = Ring::new(0);
        r.push(b"\x1b[?1049h\x9fpartial");
        assert_eq!(r.replay(), (r.contents(), false));
    }

    #[test]
    fn modes_set_in_dropped_bytes_come_first() {
        // The head is dropped whole; the ring then holds dots and "end".
        let mut r = Ring::new(0);
        let cap = r.capacity();
        r.push(b"\x1b[?1049h\x1b[?2004h");
        r.push(&vec![b'.'; cap]);
        r.push(b"end");
        let (bytes, truncated) = r.replay();
        assert!(truncated);
        let mut want = b"\x1b[?1049h\x1b[?2004h".to_vec();
        want.extend(r.contents());
        assert_eq!(bytes, want);
    }

    #[test]
    fn a_mode_cleared_in_kept_bytes_ends_cleared() {
        let mut r = Ring::new(0);
        let cap = r.capacity();
        r.push(b"\x1b[?2004h\x1b[?1002h");
        r.push(&vec![b'.'; cap]);
        r.push(b"\n\x1b[?2004l\x1b[?1002lend");
        let (bytes, _) = r.replay();
        let mut e = kyko_term::Emulator::new(20, 5, 0);
        e.feed(&bytes);
        let m = e.modes();
        assert!(!m.bracketed_paste && !m.mouse_drag, "{m:?}");
    }

    #[test]
    fn a_sequence_cut_by_the_wrap_is_not_replayed() {
        // Dropping exactly `cap - 2` bytes cuts `CSI ? 1049 h` after `CSI ?`: the ring then
        // starts with "1049h", and no line break follows.
        let mut r = Ring::new(0);
        let cap = r.capacity();
        let mut first = vec![b'x'; cap - 5];
        first.extend_from_slice(b"\x1b[?10");
        r.push(&first);
        let mut second = b"49h".to_vec();
        second.extend(vec![b'd'; cap - 5]);
        r.push(&second);
        assert!(r.contents().starts_with(b"1049hd"));
        let (bytes, truncated) = r.replay();
        assert!(truncated);
        let mut want = b"\x1b[?1049h".to_vec();
        want.extend(vec![b'd'; cap - 5]);
        assert_eq!(bytes, want);
    }

    #[test]
    fn a_character_cut_by_the_wrap_is_not_replayed() {
        // Dropping `cap - 3` bytes leaves the emoji's last three bytes at the head.
        let mut r = Ring::new(0);
        let cap = r.capacity();
        let mut first = vec![b'x'; cap - 4];
        first.extend_from_slice("\u{1F600}".as_bytes());
        r.push(&first);
        r.push(&vec![b'z'; cap - 3]);
        assert_eq!(&r.contents()[..3], &"\u{1F600}".as_bytes()[1..]);
        let (bytes, _) = r.replay();
        assert_eq!(bytes, vec![b'z'; cap - 3]);
    }

    #[test]
    fn line_oriented_output_starts_on_a_whole_line() {
        // Lines of 11 bytes, and a capacity that is not a multiple of 11: the cut falls inside
        // a line.
        let mut r = Ring::new(0);
        let lines: Vec<u8> = (0..30_000)
            .flat_map(|i| format!("line {i:05}\n").into_bytes())
            .collect();
        r.push(&lines);
        assert!(!r.capacity().is_multiple_of(11));
        let (bytes, truncated) = r.replay();
        assert!(truncated);
        assert!(bytes.starts_with(b"line "), "{:?}", &bytes[..20]);
        assert!(bytes.ends_with(b"line 29999\n"));
    }

    #[test]
    fn one_push_larger_than_capacity_feeds_the_modes_in_order() {
        let mut r = Ring::new(0);
        r.push(b"\x1b[?1h");
        let mut big = b"\x1b[?1l\x1b[?2004h".to_vec();
        big.extend(vec![b'y'; r.capacity()]);
        r.push(&big);
        let (bytes, truncated) = r.replay();
        assert!(truncated);
        assert!(bytes.starts_with(b"\x1b[?2004hyyy"), "{:?}", &bytes[..16]);
    }
}
