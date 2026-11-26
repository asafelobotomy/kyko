//! [`Workspace`]: the window's handle on the session thread, every call a command sent to it.

use super::*;

impl Workspace {
    /// Starts the thread. The callbacks run on it: `on_frames` when a pane's screen changed,
    /// `on_layout` when the tabs did, `on_hide` when the drop-down should hide (its last tab
    /// closed), `on_quit` when a windowed kyko has nothing left to show, `on_bell` when a
    /// pane's program rang the bell.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        socket: PathBuf,
        mut launch: Launch,
        look: SharedLook,
        on_frames: Box<dyn Fn() + Send>,
        on_layout: Box<dyn Fn() + Send>,
        on_hide: Box<dyn Fn() + Send>,
        on_quit: Box<dyn Fn() + Send>,
        on_copy: Box<dyn Fn(String, bool) + Send>,
        on_bell: Box<dyn Fn(PaneId) + Send>,
    ) -> Workspace {
        let (tx, rx) = channel();
        let rc_tx = tx.clone();
        let rc_path = crate::remote::serve(&crate::remote::socket_path(), move |req| {
            let (reply_tx, reply_rx) = channel();
            if rc_tx.send(Cmd::Remote(req, reply_tx)).is_err() {
                return kyko_core::remote::Reply::err("the window is closing");
            }
            reply_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap_or_else(|_| kyko_core::remote::Reply::err("the window did not answer"))
        });
        let shared = Arc::new(Mutex::new(Shared::default()));
        let colors: SharedColors = Arc::new(Mutex::new(look.lock().unwrap().colors.clone()));
        let startup = Mutex::new(std::mem::take(&mut launch.startup));
        let ctx = Ctx {
            socket,
            launch,
            colors: colors.clone(),
            look,
            shared: shared.clone(),
            on_frames,
            on_layout,
            on_hide,
            on_quit,
            on_copy,
            on_bell,
            rc_path,
            startup,
        };
        let thread = std::thread::Builder::new()
            .name("kyko-link".into())
            .spawn(move || thread_main(ctx, rx))
            .expect("thread starts");
        Workspace {
            tx,
            colors,
            shared,
            thread: Mutex::new(Some(thread)),
        }
    }

    pub fn input(&self, pane: PaneId, bytes: Vec<u8>) {
        let _ = self.tx.send(Cmd::Input(pane, bytes));
    }

    /// Keys typed in `pane`: they reach every pane of its tab while the tab broadcasts.
    pub fn typed(&self, pane: PaneId, bytes: Vec<u8>) {
        let _ = self.tx.send(Cmd::Typed(pane, bytes));
    }

    pub fn search(&self, op: SearchOp) {
        let _ = self.tx.send(Cmd::Search(op));
    }

    pub fn mouse(&self, pane: PaneId, ev: MouseEv) {
        let _ = self.tx.send(Cmd::Mouse(pane, ev));
    }

    /// Puts the pane's selection on the clipboard.
    pub fn copy(&self, pane: PaneId) {
        let _ = self.tx.send(Cmd::Copy(pane));
    }

    /// Scrolls the pane to the previous (`dir < 0`) or next prompt.
    pub fn jump_prompt(&self, pane: PaneId, dir: i32) {
        let _ = self.tx.send(Cmd::PromptJump(pane, dir));
    }

    /// Selects what the last command printed and puts it on the clipboard.
    pub fn scroll_to(&self, pane: PaneId, offset: u32) {
        let _ = self.tx.send(Cmd::ScrollTo(pane, offset));
    }

    pub fn show_scrollback(&self, pane: PaneId) {
        let _ = self.tx.send(Cmd::ShowScrollback(pane));
    }

    pub fn copy_last_output(&self, pane: PaneId) {
        let _ = self.tx.send(Cmd::CopyLastOutput(pane));
    }

    /// Scrolls the pane to the previous (`dir < 0`) or next command that failed.
    pub fn jump_failed(&self, pane: PaneId, dir: i32) {
        let _ = self.tx.send(Cmd::FailedJump(pane, dir));
    }

    pub fn copy_last_command(&self, pane: PaneId) {
        let _ = self.tx.send(Cmd::CopyLastCommand(pane));
    }

    pub fn paste(&self, pane: PaneId, text: String) {
        let _ = self.tx.send(Cmd::Paste(pane, text));
    }

    pub fn resize(&self, pane: PaneId, cols: u16, rows: u16) {
        let _ = self.tx.send(Cmd::Resize(pane, cols, rows));
    }

    /// The cell size in pixels. One font serves the window, so it holds for every pane.
    pub fn cell_px(&self, w: u16, h: u16) {
        self.shared.lock().unwrap().cell_px = Some((w, h));
        let _ = self.tx.send(Cmd::CellPx(w, h));
    }

    pub fn act(&self, act: Act) {
        let _ = self.tx.send(Cmd::Act(act));
    }

    /// Draws with these colours from the next change on, and asks for one.
    pub fn set_colors(&self, colors: Colors) {
        *self.colors.lock().unwrap() = colors;
        let _ = self.tx.send(Cmd::Redraw);
    }

    /// Tells the thread to end the sessions (a windowed kyko) and stop.
    pub fn request_end(&self) {
        let _ = self.tx.send(Cmd::End);
    }

    /// Waits for the thread to finish, after [`Workspace::request_end`].
    pub fn wait(&self) {
        if let Some(t) = self.thread.lock().unwrap().take() {
            let _ = t.join();
        }
    }
}
