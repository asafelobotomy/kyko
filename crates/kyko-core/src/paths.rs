//! Where kyko's config and uQ's theme tokens live, resolved as uQ resolves them
//! (`uq-core/src/paths.rs`), so kyko reads the same `tokens.json` uQ writes. The environment is a
//! function, so tests do not touch the process's. An empty variable counts as unset.

use std::ffi::OsString;
use std::path::PathBuf;

fn base(env: &impl Fn(&str) -> Option<OsString>, xdg: &str, under_home: &str) -> Option<PathBuf> {
    let get = |k: &str| env(k).filter(|v| !v.is_empty());
    get(xdg)
        .map(PathBuf::from)
        .or_else(|| get("HOME").map(|h| PathBuf::from(h).join(under_home)))
}

/// `<config>/kyko/config.toml`: `$XDG_CONFIG_HOME`, else `$HOME/.config`.
pub fn config_path(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    base(&env, "XDG_CONFIG_HOME", ".config").map(|d| d.join("kyko/config.toml"))
}

/// `<state>/uq/theme/tokens.json`: `$XDG_STATE_HOME` (only an absolute one, as uQ), else
/// `$HOME/.local/state`. uQ's own rule, from the vendored uq-consts.
pub fn uq_tokens_path(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    use crate::uq_consts::paths;
    paths::state_home_from(&env).map(|d| d.join(paths::TOKENS))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| {
            vars.iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| OsString::from(v))
        }
    }

    #[test]
    fn config() {
        let p = |vars: &[(&str, &str)]| config_path(env(vars));
        assert_eq!(
            p(&[("HOME", "/home/a")]),
            Some("/home/a/.config/kyko/config.toml".into())
        );
        assert_eq!(
            p(&[("HOME", "/home/a"), ("XDG_CONFIG_HOME", "/x")]),
            Some("/x/kyko/config.toml".into())
        );
        assert_eq!(
            p(&[("XDG_CONFIG_HOME", "/x")]),
            Some("/x/kyko/config.toml".into())
        );
        assert_eq!(
            p(&[("HOME", "/home/a"), ("XDG_CONFIG_HOME", "")]),
            Some("/home/a/.config/kyko/config.toml".into())
        );
        assert_eq!(p(&[]), None);
        assert_eq!(p(&[("HOME", "")]), None);
    }

    #[test]
    fn tokens() {
        let p = |vars: &[(&str, &str)]| uq_tokens_path(env(vars));
        assert_eq!(
            p(&[("HOME", "/home/a")]),
            Some("/home/a/.local/state/uq/theme/tokens.json".into())
        );
        assert_eq!(
            p(&[("HOME", "/home/a"), ("XDG_STATE_HOME", "/s")]),
            Some("/s/uq/theme/tokens.json".into())
        );
        assert_eq!(p(&[("XDG_CONFIG_HOME", "/x")]), None);
        // uQ ignores a relative XDG_STATE_HOME, so kyko does too.
        assert_eq!(
            p(&[("HOME", "/home/a"), ("XDG_STATE_HOME", "s")]),
            Some("/home/a/.local/state/uq/theme/tokens.json".into())
        );
    }
}
