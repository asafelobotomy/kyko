//! The escape-sequence scanner's state: where in a sequence it is, and the parameters and
//! intermediates collected so far.

use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum State {
    #[default]
    Ground,
    Escape,
    EscapeIntermediate,
    CsiEntry,
    CsiParam,
    CsiIntermediate,
    CsiIgnore,
    DcsEntry,
    DcsParam,
    DcsIntermediate,
    DcsIgnore,
    DcsPassthrough,
    Osc,
    SosPmApc,
}

/// The parser: `vte`'s states and what a CSI collects.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Scan {
    pub(crate) state: State,
    pub(crate) intermediates: [u8; MAX_INTERMEDIATES],
    pub(crate) n_intermediates: usize,
    pub(crate) ignoring: bool,
    /// Values pushed so far, parameters and subparameters together (`vte`'s `Params::len`).
    pub(crate) n_values: usize,
    /// The first value of each parameter, which is all alacritty reads for these sequences.
    pub(crate) firsts: [u16; MAX_PARAMS],
    pub(crate) n_params: usize,
    /// The current parameter already has subparameters, so its first value is recorded.
    pub(crate) in_subparams: bool,
    pub(crate) param: u16,
}

impl Scan {
    pub(crate) fn reset(&mut self) {
        self.n_intermediates = 0;
        self.ignoring = false;
        self.n_values = 0;
        self.n_params = 0;
        self.in_subparams = false;
        self.param = 0;
    }

    pub(crate) fn intermediates(&self) -> &[u8] {
        &self.intermediates[..self.n_intermediates]
    }

    pub(crate) fn collect(&mut self, byte: u8) {
        if self.n_intermediates == MAX_INTERMEDIATES {
            self.ignoring = true;
        } else {
            self.intermediates[self.n_intermediates] = byte;
            self.n_intermediates += 1;
        }
    }

    pub(crate) fn full(&self) -> bool {
        self.n_values == MAX_PARAMS
    }

    /// `vte`'s `Params::push`: ends the current parameter with `value`.
    pub(crate) fn push(&mut self) {
        if !self.in_subparams {
            self.firsts[self.n_params] = self.param;
            self.n_params += 1;
        }
        self.in_subparams = false;
        self.n_values += 1;
    }

    /// `vte`'s `Params::extend`: `value` is followed by a subparameter.
    pub(crate) fn extend(&mut self) {
        if !self.in_subparams {
            self.firsts[self.n_params] = self.param;
            self.n_params += 1;
        }
        self.in_subparams = true;
        self.n_values += 1;
    }

    pub(crate) fn param_next(&mut self, byte: u8) {
        if self.full() {
            self.ignoring = true;
        } else {
            self.param = self
                .param
                .saturating_mul(10)
                .saturating_add(u16::from(byte - b'0'));
        }
    }

    pub(crate) fn param_end(&mut self) {
        if self.full() {
            self.ignoring = true;
        } else {
            self.push();
            self.param = 0;
        }
    }

    pub(crate) fn subparam(&mut self) {
        if self.full() {
            self.ignoring = true;
        } else {
            self.extend();
            self.param = 0;
        }
    }

    /// The parameters' first values, as alacritty's `next_param_or` reads them.
    pub(crate) fn params(&self) -> &[u16] {
        &self.firsts[..self.n_params]
    }
}
