//! Cursor reader and writers for the wire encoding. All integers are
//! little-endian. Every read checks the bytes remaining first, so nothing is
//! ever allocated from a length the input has not backed with real bytes.

use crate::ProtoError;

pub(crate) struct Reader<'a> {
    rest: &'a [u8],
}

impl<'a> Reader<'a> {
    pub(crate) fn new(body: &'a [u8]) -> Self {
        Self { rest: body }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], ProtoError> {
        if self.rest.len() < n {
            return Err(ProtoError::Malformed("truncated frame"));
        }
        let (head, tail) = self.rest.split_at(n);
        self.rest = tail;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], ProtoError> {
        let mut a = [0u8; N];
        a.copy_from_slice(self.take(N)?);
        Ok(a)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, ProtoError> {
        Ok(self.array::<1>()?[0])
    }

    pub(crate) fn u16(&mut self) -> Result<u16, ProtoError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, ProtoError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    pub(crate) fn i32(&mut self) -> Result<i32, ProtoError> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    pub(crate) fn bool(&mut self) -> Result<bool, ProtoError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(ProtoError::Malformed("bad boolean")),
        }
    }

    /// A `u32` length then that many bytes, borrowed from the frame.
    pub(crate) fn bytes(&mut self) -> Result<&'a [u8], ProtoError> {
        let n = self.u32()? as usize;
        self.take(n)
    }

    pub(crate) fn string(&mut self) -> Result<String, ProtoError> {
        std::str::from_utf8(self.bytes()?)
            .map(str::to_owned)
            .map_err(|_| ProtoError::Malformed("invalid UTF-8"))
    }

    /// A `u32` count then that many items. `min_item` is the smallest encoded
    /// size of one item; a count the remaining bytes cannot hold is rejected
    /// before anything is allocated.
    pub(crate) fn list<T>(
        &mut self,
        min_item: usize,
        mut item: impl FnMut(&mut Self) -> Result<T, ProtoError>,
    ) -> Result<Vec<T>, ProtoError> {
        let n = self.u32()? as usize;
        if n > self.rest.len() / min_item.max(1) {
            return Err(ProtoError::Malformed("list longer than frame"));
        }
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(item(self)?);
        }
        Ok(v)
    }

    pub(crate) fn finish(self) -> Result<(), ProtoError> {
        if self.rest.is_empty() {
            Ok(())
        } else {
            Err(ProtoError::Malformed("trailing bytes"))
        }
    }
}

pub(crate) fn put_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn put_i32(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn put_bool(out: &mut Vec<u8>, v: bool) {
    out.push(u8::from(v));
}

pub(crate) fn put_bytes(out: &mut Vec<u8>, b: &[u8]) {
    put_u32(out, b.len() as u32);
    out.extend_from_slice(b);
}

pub(crate) fn put_str(out: &mut Vec<u8>, s: &str) {
    put_bytes(out, s.as_bytes());
}

/// Run `body` to write a frame's type byte and body after a length
/// placeholder, then fill in the length.
pub(crate) fn framed(out: &mut Vec<u8>, kind: u8, body: impl FnOnce(&mut Vec<u8>)) {
    let start = out.len();
    put_u32(out, 0);
    out.push(kind);
    body(out);
    let len = (out.len() - start - 4) as u32;
    out[start..start + 4].copy_from_slice(&len.to_le_bytes());
}
