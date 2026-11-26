//! Reading `/proc`: the processes a test started, and whether they have ended.

use super::*;

pub struct Proc {
    pub pid: i32,
    pub ppid: i32,
    pub pgrp: i32,
    pub session: i32,
    pub state: char,
    pub cmdline: String,
}

/// Every live process this user can see, read from `/proc`. Zombies are left
/// out: a process that has ended is gone, whether or not something has reaped
/// it yet. Where PID 1 does not reap orphans (a CI job's container), an orphan
/// that has ended stays a zombie for good.
pub fn processes() -> Vec<Proc> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return out;
    };
    for entry in dir.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<i32>().ok())
        else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            continue;
        };
        // The fields after the command name, which is in parentheses and may hold spaces.
        let Some(rest) = stat.rsplit_once(')').map(|(_, r)| r) else {
            continue;
        };
        let f: Vec<&str> = rest.split_whitespace().collect();
        if f.len() < 4 {
            continue;
        }
        let cmdline = std::fs::read(format!("/proc/{pid}/cmdline"))
            .map(|b| {
                String::from_utf8_lossy(&b)
                    .trim_end_matches('\0')
                    .replace('\0', " ")
            })
            .unwrap_or_default();
        let state = f[0].chars().next().unwrap_or('?');
        if state == 'Z' {
            continue;
        }
        out.push(Proc {
            pid,
            state,
            ppid: f[1].parse().unwrap_or(0),
            pgrp: f[2].parse().unwrap_or(0),
            session: f[3].parse().unwrap_or(0),
            cmdline,
        });
    }
    out
}

/// SIGKILL to every process in any of these sessions.
pub fn kill_sessions(sessions: &[i32]) {
    if sessions.is_empty() {
        return;
    }
    for &s in sessions {
        let _ = killpg(Pid::from_raw(s), Signal::SIGKILL);
    }
    for p in processes() {
        if sessions.contains(&p.session) {
            let _ = kill(Pid::from_raw(p.pid), Signal::SIGKILL);
        }
    }
}

/// CPU time (user plus system, in clock ticks) the process has used so far.
pub fn cpu_ticks(pid: i32) -> u64 {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    let rest = stat.rsplit_once(')').unwrap().1;
    let f: Vec<&str> = rest.split_whitespace().collect();
    // Fields 14 and 15 of stat; `rest` starts at field 3.
    f[11].parse::<u64>().unwrap() + f[12].parse::<u64>().unwrap()
}

/// The state letter of `pid` from `/proc/<pid>/stat` (`R`, `S`, `Z`, ...), or
/// `None` once the process is reaped.
pub fn state(pid: i32) -> Option<char> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = stat.rsplit_once(')')?.1;
    rest.split_whitespace().next()?.chars().next()
}

/// True while the process runs. A zombie has ended, so it counts as gone:
/// where PID 1 does not reap orphans (a CI job's container), an orphaned
/// process that has ended stays a zombie for good.
pub fn exists(pid: i32) -> bool {
    !matches!(state(pid), None | Some('Z'))
}

/// Waits until `pid` has ended (reaped, or a zombie: see [`exists`]).
pub fn gone_within(pid: i32, limit: Duration) -> bool {
    let end = Instant::now() + limit;
    while exists(pid) {
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}
