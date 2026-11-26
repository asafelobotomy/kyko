//! Ending the sessions when `kykod` stops: hang up every shell, wait, then kill what is left.

use super::*;

impl Server {
    /// Ends every session and exits the process with status 0, unless `force`
    /// is false and some pane is busy, or the sessions are already being
    /// ended; then it returns why and changes nothing.
    ///
    /// The decision is one pass under the pane table's lock: every pane is
    /// checked, then either nothing happens or the server is marked as ending
    /// and the list of panes taken. Nothing is hung up until all are checked.
    /// Input comes only from the connection's thread, which is the one making
    /// this call for `EndSessions`, so no new command can be typed between the
    /// check and the hang-up; input already queued, or a background job
    /// taking the terminal, in the moment between them is not seen.
    ///
    /// Then, with no lock held: the socket file is removed, so no window can
    /// reach a holder that is going away; every shell's process group gets
    /// `SIGHUP` at once; one `poll` on their pidfds, and on those of panes
    /// still being closed (already hung up by `Close`), waits until all have
    /// ended or [`END_GRACE`] has passed. At that point any pane still being
    /// closed whose shell is running gets `SIGKILL` to its process group, as
    /// `Close` promised, since exiting ends its close thread, and up to
    /// [`KILL_REAP`] more is given for those to end. The process then exits 0,
    /// after reaping every shell that has ended by then (a non-blocking wait on
    /// each). A shell of a pane that was not closed and is still running is
    /// not killed: it outlives `kykod`, and the kernel hangs its terminal up
    /// when `kykod`'s side closes.
    ///
    /// The lock file is left behind on purpose. Removing it would let a
    /// `kykod` that had opened it just before lock a file no longer in the
    /// folder while a third one creates and locks a new one, and both would
    /// then serve the same socket. The lock itself is released by the kernel
    /// when this process exits.
    pub(crate) fn end_sessions(&self, force: bool, why: &str) -> NotEnded {
        let (panes, closing): (Vec<Arc<Pane>>, Vec<Arc<Pane>>) = {
            let table = self.table();
            if self.ending.load(Ordering::Acquire) {
                return NotEnded::AlreadyEnding;
            }
            if !force {
                // The table is ordered by id, so the list is ascending.
                let busy: Vec<PaneId> = table
                    .values()
                    .filter(|p| p.is_busy())
                    .map(|p| p.id())
                    .collect();
                if !busy.is_empty() {
                    return NotEnded::Busy(busy);
                }
            }
            self.ending.store(true, Ordering::Release);
            let closing = lock_list(&self.closing).clone();
            (table.values().cloned().collect(), closing)
        };
        log!(
            "ending every session ({why}): hanging up {} shell(s), {} more being closed",
            panes.len(),
            closing.len()
        );
        if let Err(e) = fs::remove_file(&self.path) {
            problem!(
                WARN,
                "[KYKO-015] cannot remove {}: {e}",
                self.path.display()
            );
        }
        for pane in &panes {
            pane.hangup();
        }
        let end = Instant::now() + END_GRACE;
        let fds: Vec<OwnedFd> = panes
            .iter()
            .chain(&closing)
            .filter_map(|p| match p.exit_fd() {
                Ok(fd) => fd,
                Err(e) => {
                    problem!(
                        WARN,
                        "[KYKO-014] pane {}: cannot wait for its shell: {e}",
                        p.id()
                    );
                    None
                }
            })
            .collect();
        let left = wait_for_exits(fds, end);
        if left > 0 {
            problem!(
                WARN,
                "[KYKO-015] {left} shell(s) still running after {}s; exiting anyway",
                END_GRACE.as_secs()
            );
        }
        // Closed panes: the list as it is now, which also holds any pane closed
        // since the decision. A close thread that has finished took its pane
        // out, and one that has not is ended with the process.
        let still_closing = lock_list(&self.closing).clone();
        let killed: Vec<OwnedFd> = still_closing
            .iter()
            .filter(|p| p.kill_if_running())
            .filter_map(|p| p.exit_fd().ok().flatten())
            .collect();
        // A killed shell ends at once unless it is stuck in the kernel; it
        // gets a moment, so that it can be reaped below.
        wait_for_exits(killed, Instant::now() + KILL_REAP);
        // Reap every shell that has ended, without waiting for any: once
        // `kykod` has gone, an unreaped one would be left to PID 1, and not
        // every PID 1 reaps (a container's often does not).
        for pane in panes.iter().chain(&closing).chain(&still_closing) {
            pane.reap();
        }
        log!("exiting");
        std::process::exit(0);
    }
}
