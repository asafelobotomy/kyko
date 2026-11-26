//! The reader behind [`Config::parse`]: typed reads of one key at a time, each bad value a
//! warning and the default in its place.

use super::*;

/// 1-based line and character column of byte `at`.
pub(crate) fn line_col(text: &str, at: usize) -> (usize, usize) {
    let at = at.min(text.len());
    let at = (0..=at)
        .rev()
        .find(|i| text.is_char_boundary(*i))
        .unwrap_or(0);
    let before = &text[..at];
    let line = before.matches('\n').count() + 1;
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    (line, before[start..].chars().count() + 1)
}

pub(crate) const SECTIONS: &[&str] = &[
    "hotkey", "window", "theme", "terminal", "sessions", "keys", "advanced",
];

#[derive(Default)]
pub(crate) struct Reader {
    pub(crate) warnings: Vec<Warning>,
}

/// One section: its table, its dotted name, and the keys read from it so far.
pub(crate) struct Section<'a> {
    pub(crate) name: &'a str,
    pub(crate) table: Option<&'a Table>,
}

impl<'a> Section<'a> {
    pub(crate) fn get(&self, key: &str) -> Option<&'a Value> {
        self.table.and_then(|t| t.get(key))
    }
}

impl Reader {
    pub(crate) fn warn(&mut self, key: impl Into<String>, message: impl Into<String>) {
        self.warnings.push(Warning::new(key, message));
    }

    pub(crate) fn section<'a>(
        &mut self,
        parent: &'a Table,
        name: &'a str,
        dotted: &'a str,
        known: &[&str],
    ) -> Section<'a> {
        let table = match parent.get(name) {
            None => None,
            Some(Value::Table(t)) => {
                for key in t.keys() {
                    if !known.contains(&key.as_str()) {
                        self.warn(
                            format!("{dotted}.{key}"),
                            format!("unknown key {dotted}.{key}"),
                        );
                    }
                }
                Some(t)
            }
            Some(_) => {
                self.warn(dotted, "not a table");
                None
            }
        };
        Section {
            name: dotted,
            table,
        }
    }

    pub(crate) fn bool(&mut self, s: &Section, key: &str, default: bool) -> bool {
        match s.get(key) {
            None => default,
            Some(Value::Boolean(b)) => *b,
            Some(_) => {
                self.warn(format!("{}.{key}", s.name), "not true or false");
                default
            }
        }
    }

    pub(crate) fn int(
        &mut self,
        s: &Section,
        key: &str,
        range: RangeInclusive<i64>,
        default: u32,
    ) -> u32 {
        self.opt_int(s, key, range).unwrap_or(default)
    }

    pub(crate) fn opt_int(
        &mut self,
        s: &Section,
        key: &str,
        range: RangeInclusive<i64>,
    ) -> Option<u32> {
        match s.get(key)? {
            Value::Integer(n) if range.contains(n) => u32::try_from(*n).ok(),
            Value::Integer(n) => {
                self.warn(
                    format!("{}.{key}", s.name),
                    format!("{n} is outside {}–{}", range.start(), range.end()),
                );
                None
            }
            _ => {
                self.warn(format!("{}.{key}", s.name), "not an integer");
                None
            }
        }
    }

    pub(crate) fn float(
        &mut self,
        s: &Section,
        key: &str,
        range: RangeInclusive<f64>,
        default: f64,
    ) -> f64 {
        let v = match s.get(key) {
            None => return default,
            Some(Value::Float(f)) => *f,
            Some(Value::Integer(n)) => *n as f64,
            Some(_) => {
                self.warn(format!("{}.{key}", s.name), "not a number");
                return default;
            }
        };
        if v.is_finite() && range.contains(&v) {
            v
        } else {
            self.warn(
                format!("{}.{key}", s.name),
                format!("{v} is outside {}–{}", range.start(), range.end()),
            );
            default
        }
    }

    pub(crate) fn string<'a>(&mut self, s: &Section<'a>, key: &str) -> Option<&'a str> {
        match s.get(key)? {
            Value::String(v) => Some(v),
            _ => {
                self.warn(format!("{}.{key}", s.name), "not a string");
                None
            }
        }
    }

    pub(crate) fn choice<T: Copy>(
        &mut self,
        s: &Section,
        key: &str,
        options: &[(&str, T)],
        default: T,
    ) -> T {
        let Some(v) = self.string(s, key) else {
            return default;
        };
        match options.iter().find(|(n, _)| *n == v) {
            Some((_, t)) => *t,
            None => {
                let names: Vec<&str> = options.iter().map(|(n, _)| *n).collect();
                self.warn(
                    format!("{}.{key}", s.name),
                    format!("{v:?} is not one of {}", names.join(", ")),
                );
                default
            }
        }
    }

    /// A key sequence, `""` for none. `Err(())` when the value is bad (already warned).
    pub(crate) fn seq(&mut self, s: &Section, key: &str) -> Option<Result<Option<KeySeq>, ()>> {
        let v = match s.get(key)? {
            Value::String(v) => v,
            _ => {
                self.warn(format!("{}.{key}", s.name), "not a string");
                return Some(Err(()));
            }
        };
        if v.is_empty() {
            return Some(Ok(None));
        }
        Some(match v.parse::<KeySeq>() {
            Ok(seq) => Ok(Some(seq)),
            Err(e) => {
                self.warn(format!("{}.{key}", s.name), e.0);
                Err(())
            }
        })
    }

    pub(crate) fn colour(&mut self, s: &Section, key: &str) -> Option<u32> {
        let v = self.string(s, key)?;
        let c = parse_rgb(v);
        if c.is_none() {
            self.warn(format!("{}.{key}", s.name), format!("{v:?} is not #rrggbb"));
        }
        c
    }
}
