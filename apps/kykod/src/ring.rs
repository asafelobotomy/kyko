//! A byte ring that keeps the newest output of a pane.

use std::collections::VecDeque;

const MIN_CAPACITY: usize = 256 * 1024;
const MAX_CAPACITY: usize = 16 * 1024 * 1024;

/// The most recent bytes a pane produced, up to a fixed capacity.
pub struct Ring {
    buf: VecDeque<u8>,
    capacity: usize,
    wrapped: bool,
}

impl Ring {
    /// A ring holding up to `capacity` bytes, clamped to 256 KiB ..= 16 MiB.
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.clamp(MIN_CAPACITY, MAX_CAPACITY);
        Ring {
            buf: VecDeque::new(),
            capacity,
            wrapped: false,
        }
    }

    /// Appends `bytes`, dropping the oldest bytes beyond the capacity.
    pub fn push(&mut self, bytes: &[u8]) {
        let bytes = if bytes.len() > self.capacity {
            self.wrapped = true;
            &bytes[bytes.len() - self.capacity..]
        } else {
            bytes
        };
        let overflow = (self.buf.len() + bytes.len()).saturating_sub(self.capacity);
        if overflow > 0 {
            self.buf.drain(..overflow);
            self.wrapped = true;
        }
        self.buf.extend(bytes);
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
}
