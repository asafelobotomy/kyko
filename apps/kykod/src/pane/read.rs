//! Waiting for and reading the shell's output: one batch at a time, without blocking past a
//! deadline.

use super::*;

/// What [`wait`] found.
pub(crate) enum Wake {
    /// The terminal has output, or has reported its end or an error: a read
    /// tells which.
    Output,
    /// The shell has ended.
    Exited,
    /// Nothing came within the limit.
    Quiet,
}

/// Waits until the terminal can be read or the shell behind `exit` has ended,
/// for at most `limit` if one is given. An ended shell is reported first when
/// both are ready. An error of `poll` itself counts as output, so the read
/// that follows reports it.
pub(crate) fn wait(reader: &File, exit: Option<&OwnedFd>, limit: Option<Duration>) -> Wake {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    let timeout = limit.and_then(|l| Timespec::try_from(l).ok());
    let mut fds = [
        PollFd::new(reader, PollFlags::IN),
        PollFd::new(reader, PollFlags::IN),
    ];
    let n = match exit {
        Some(fd) => {
            fds[1] = PollFd::new(fd, PollFlags::IN);
            2
        }
        None => 1,
    };
    loop {
        match poll(&mut fds[..n], timeout.as_ref()) {
            Ok(0) => return Wake::Quiet,
            Ok(_) if n == 2 && !fds[1].revents().is_empty() => return Wake::Exited,
            Ok(_) => return Wake::Output,
            // A signal is rare here; waiting the full limit again is harmless.
            Err(rustix::io::Errno::INTR) => {}
            Err(_) => return Wake::Output,
        }
    }
}

/// One read, which may block, then whatever else is already waiting, up to
/// the size of `buf`. `Ok(0)` is the end of the terminal.
pub(crate) fn read_batch(reader: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = reader.read(buf)?;
    if n == 0 {
        return Ok(0);
    }
    while n < buf.len() && readable_within(reader, Duration::ZERO) {
        match reader.read(&mut buf[n..]) {
            Ok(m) if m > 0 => n += m,
            // The next read sees the end or the error again and handles it.
            _ => break,
        }
    }
    Ok(n)
}

/// True if `reader` has something to read (or has hung up) within `limit`;
/// false once `limit` has passed with nothing. An error counts as readable, so
/// the read that follows reports it.
pub(crate) fn readable_within(reader: &File, limit: Duration) -> bool {
    let Ok(timeout) = rustix::event::Timespec::try_from(limit) else {
        return true;
    };
    let mut fds = [rustix::event::PollFd::new(
        reader,
        rustix::event::PollFlags::IN,
    )];
    loop {
        match rustix::event::poll(&mut fds, Some(&timeout)) {
            Ok(n) => return n > 0,
            // A signal is rare here; waiting the full limit again is harmless.
            Err(rustix::io::Errno::INTR) => {}
            Err(_) => return true,
        }
    }
}
