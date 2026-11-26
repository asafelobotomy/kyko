//! Waiting for the shells to exit: the descriptors that say a pane's process has ended.

use super::*;

pub(crate) fn lock_list(list: &Mutex<Vec<Arc<Pane>>>) -> MutexGuard<'_, Vec<Arc<Pane>>> {
    list.lock().unwrap_or_else(|p| p.into_inner())
}

/// Waits until every pidfd in `fds` is readable (its process has ended) or
/// `end` has passed, sleeping in one `poll` on all of them at a time. Returns
/// how many processes had not ended.
pub(crate) fn wait_for_exits(mut fds: Vec<OwnedFd>, end: Instant) -> usize {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    while !fds.is_empty() {
        let left = end.saturating_duration_since(Instant::now());
        let Ok(timeout) = Timespec::try_from(left) else {
            break;
        };
        if left.is_zero() {
            break;
        }
        let mut polls: Vec<PollFd<'_>> = fds
            .iter()
            .map(|fd| PollFd::new(fd, PollFlags::IN))
            .collect();
        match poll(&mut polls, Some(&timeout)) {
            Ok(0) => break,
            Ok(_) => {
                let ended: Vec<bool> = polls.iter().map(|p| !p.revents().is_empty()).collect();
                let mut i = 0;
                fds.retain(|_| {
                    i += 1;
                    !ended[i - 1]
                });
            }
            // A second signal, say: wait again for what is left of the grace.
            Err(rustix::io::Errno::INTR) => {}
            Err(e) => {
                problem!(WARN, "[KYKO-015] cannot wait for the shells: {e}");
                break;
            }
        }
    }
    fds.len()
}
