//! Desktop notifications: what a program asks for, and the commands that ended while the user
//! looked away, cut to a few in a burst.

use super::*;

/// Shows a notification a program asked for (OSC 9, 777, 99) on the desktop's notification
/// service, off the session thread so a slow service never holds the terminal up.
/// Whether a command that ran for `ran` earns a notification, the pane being `seen` or not.
pub(crate) fn finish_worth_telling(
    when: NotifyOnFinish,
    after: u32,
    ran: Duration,
    seen: bool,
) -> bool {
    ran.as_secs() >= u64::from(after)
        && match when {
            NotifyOnFinish::Never => false,
            NotifyOnFinish::Unfocused => !seen,
            NotifyOnFinish::Always => true,
        }
}

/// "Finished after 1m 05s", or "Failed (exit 2) after 12s".
pub(crate) fn finished_text(ran: Duration, status: u8) -> String {
    let s = ran.as_secs();
    let took = match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m {:02}s", s / 60, s % 60),
        _ => format!("{}h {:02}m", s / 3600, s % 3600 / 60),
    };
    if status == 0 {
        format!("Finished after {took}")
    } else {
        format!("Failed (exit {status}) after {took}")
    }
}

/// How many recent command lines the command palette keeps.
pub(crate) const RECENT: usize = 50;

/// At most this many program notifications in [`NOTIFY_WINDOW`]; a program that prints them in
/// a loop gets the first few shown and the rest dropped, instead of a thread and a bus
/// connection each.
pub(crate) const NOTIFY_BURST: usize = 5;

pub(crate) const NOTIFY_WINDOW: Duration = Duration::from_secs(10);

/// Whether one more notification may go out now, given when the recent ones went (kept here).
pub(crate) fn notify_allowed(sent: &mut std::collections::VecDeque<Instant>, now: Instant) -> bool {
    while sent
        .front()
        .is_some_and(|t| now.duration_since(*t) >= NOTIFY_WINDOW)
    {
        sent.pop_front();
    }
    if sent.len() >= NOTIFY_BURST {
        return false;
    }
    sent.push_back(now);
    true
}

pub(crate) fn notify(title: String, body: String) {
    static SENT: std::sync::Mutex<std::collections::VecDeque<Instant>> =
        std::sync::Mutex::new(std::collections::VecDeque::new());
    if !notify_allowed(&mut SENT.lock().unwrap(), Instant::now()) {
        return;
    }
    std::thread::spawn(move || {
        let summary = if title.is_empty() {
            "kyko".to_string()
        } else {
            title
        };
        let result = zbus::block_on(async {
            let bus = zbus::Connection::session().await?;
            let hints: std::collections::HashMap<&str, zbus::Variant<'_>> =
                [("desktop-entry", zbus::Variant::from("org.kyko.Kyko"))].into();
            bus.call_method(
                Some("org.freedesktop.Notifications"),
                "/org/freedesktop/Notifications",
                Some("org.freedesktop.Notifications"),
                "Notify",
                &(
                    "kyko",
                    0u32,
                    "org.kyko.Kyko",
                    summary.as_str(),
                    body.as_str(),
                    Vec::<&str>::new(),
                    hints,
                    -1i32,
                ),
            )
            .await
            .map(|_| ())
        });
        if let Err(e) = result {
            kyko_diag::problem(
                "kyko",
                kyko_diag::Level::WARN,
                &format!("[KYKO-007] notification not shown: {e}"),
            );
        }
    });
}
