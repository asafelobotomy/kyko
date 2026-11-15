//! Shared by the integration tests: a seeded generator, so a failure names the seed that
//! reproduces it.
#![allow(dead_code)]

/// xorshift64*: small, seeded, and good enough to pick test inputs.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// In `0..n`; `n` must not be 0.
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    pub fn chance(&mut self, one_in: usize) -> bool {
        self.below(one_in) == 0
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    pub fn bytes(&mut self, max_len: usize) -> Vec<u8> {
        let n = self.below(max_len + 1);
        (0..n).map(|_| self.next() as u8).collect()
    }
}

/// How many random cases to run: `default`, or `KYKO_RANDOM_CASES`.
pub fn cases(default: usize) -> usize {
    std::env::var("KYKO_RANDOM_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

use kyko_core::session::{Dir, Effect, Session, SpawnRequest, SplitAxis, TabId};
use kyko_proto::PaneId;

pub const SHELL: &str = "/bin/bash";
pub const HOME: &str = "/home/u";

/// One call on a session, as the window would make it.
#[derive(Clone, Debug)]
pub enum Op {
    NewTab(SpawnRequest),
    Split(SplitAxis),
    ClosePane(PaneId),
    CloseTab(TabId),
    Exited(PaneId, i32),
    SpawnFailed(PaneId),
    Dismiss(PaneId),
    HolderLost,
    FocusDir(Dir),
    FocusPane(PaneId),
    FocusTab(TabId),
    FocusTabIndex(usize),
    NextTab,
    PrevTab,
    SetRatio(TabId, Vec<bool>, f64),
    Rename(TabId, Option<String>),
    Info(PaneId, String, String),
}

fn pane_ids(s: &Session) -> Vec<PaneId> {
    s.tabs()
        .flat_map(|t| t.panes.into_iter().map(|p| p.id))
        .collect()
}

fn tab_ids(s: &Session) -> Vec<TabId> {
    s.tabs().map(|t| t.id).collect()
}

impl Rng {
    /// A pane id the session has, most of the time; else one it does not.
    pub fn pane(&mut self, s: &Session) -> PaneId {
        let ids = pane_ids(s);
        if ids.is_empty() || self.chance(8) {
            self.next() as u32 % 1000
        } else {
            *self.pick(&ids)
        }
    }

    pub fn tab(&mut self, s: &Session) -> TabId {
        let ids = tab_ids(s);
        if self.chance(8) {
            self.next() as u32 % 1000
        } else {
            *self.pick(&ids)
        }
    }

    pub fn op(&mut self, s: &Session) -> Op {
        let axis = |r: &mut Rng| {
            if r.chance(2) {
                SplitAxis::Side
            } else {
                SplitAxis::Stacked
            }
        };
        match self.below(20) {
            0 | 1 => Op::NewTab(SpawnRequest {
                cwd: self.chance(2).then(|| "/tmp".to_string()),
                args: if self.chance(3) {
                    vec!["-c".into(), "true".into()]
                } else {
                    vec![]
                },
                hold: self.chance(3),
            }),
            2..=4 => Op::Split(axis(self)),
            5 => Op::ClosePane(self.pane(s)),
            6 => Op::CloseTab(self.tab(s)),
            7 => Op::Exited(self.pane(s), self.next() as i32 % 256),
            8 => Op::SpawnFailed(self.pane(s)),
            9 => Op::Dismiss(self.pane(s)),
            10 if self.chance(10) => Op::HolderLost,
            10 | 11 => Op::FocusDir(*self.pick(&[Dir::Left, Dir::Right, Dir::Up, Dir::Down])),
            12 => Op::FocusPane(self.pane(s)),
            13 => Op::FocusTab(self.tab(s)),
            14 => Op::FocusTabIndex(self.below(12)),
            15 => {
                if self.chance(2) {
                    Op::NextTab
                } else {
                    Op::PrevTab
                }
            }
            16 | 17 => {
                let path = (0..self.below(4)).map(|_| self.chance(2)).collect();
                let any = (self.next() % 1000) as f64 / 1000.0;
                let ratio = *self.pick(&[
                    f64::NAN,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    0.0,
                    0.05,
                    0.95,
                    1.0,
                    -3.0,
                    0.37,
                    any,
                ]);
                Op::SetRatio(self.tab(s), path, ratio)
            }
            18 => Op::Rename(
                self.tab(s),
                self.chance(3).then(|| {
                    self.pick(&["build", "", "\u{1b}\n", "€€€", "a\tb"])
                        .repeat(1 + self.below(100))
                }),
            ),
            _ => Op::Info(
                self.pane(s),
                self.pick(&["", "bash", "vim", "cargo", "a-very-long-program-name"])
                    .to_string(),
                self.pick(&[HOME, "/", "/tmp", "/home/u/src/kyko/", ""])
                    .to_string(),
            ),
        }
    }
}

pub fn apply(s: &mut Session, op: &Op) -> Vec<Effect> {
    match op.clone() {
        Op::NewTab(req) => s.new_tab(req, SHELL).unwrap_or_default(),
        Op::Split(a) => s.split(a, SHELL).unwrap_or_default(),
        Op::ClosePane(p) => s.close_pane(p, SHELL),
        Op::CloseTab(t) => s.close_tab(t, SHELL),
        Op::Exited(p, st) => s.pane_exited(p, st, SHELL),
        Op::SpawnFailed(p) => {
            s.spawn_failed(p, "failed");
            vec![]
        }
        Op::Dismiss(p) => s.dismiss(p, SHELL),
        Op::HolderLost => {
            s.holder_lost();
            vec![]
        }
        Op::FocusDir(d) => {
            s.focus_dir(d);
            vec![]
        }
        Op::FocusPane(p) => {
            s.focus_pane(p);
            vec![]
        }
        Op::FocusTab(t) => {
            s.focus_tab(t);
            vec![]
        }
        Op::FocusTabIndex(n) => {
            s.focus_tab_index(n);
            vec![]
        }
        Op::NextTab => {
            s.next_tab();
            vec![]
        }
        Op::PrevTab => {
            s.prev_tab();
            vec![]
        }
        Op::SetRatio(t, path, r) => {
            s.set_ratio(t, &path, r);
            vec![]
        }
        Op::Rename(t, n) => {
            s.rename(t, n.as_deref());
            vec![]
        }
        Op::Info(p, prog, cwd) => {
            s.apply_info(p, &prog, &cwd);
            vec![]
        }
    }
}

/// A random session after `steps` operations.
pub fn random_session(rng: &mut Rng, steps: usize) -> Session {
    let (mut s, _) = Session::fresh(HOME, SHELL);
    for _ in 0..steps {
        let op = rng.op(&s);
        apply(&mut s, &op);
    }
    s
}
