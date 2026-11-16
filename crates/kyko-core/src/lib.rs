#![forbid(unsafe_code)]

pub mod config;
pub mod configedit;
pub mod integration;
pub mod keys;
pub mod layout;
pub mod paths;
pub mod prefs;
pub mod runq;
pub mod session;
pub mod tabsearch;
pub mod theme;

/// A problem with one setting or one field, which fell back to its default. The window logs
/// each distinct warning once and shows it on the settings page.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Warning {
    /// The dotted key: `window.height_pct`, `colors.bg`.
    pub key: String,
    pub message: String,
}

impl Warning {
    pub fn new(key: impl Into<String>, message: impl Into<String>) -> Self {
        Warning {
            key: key.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Warning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.key, self.message)
    }
}

/// The workspace version, shared by every kyko binary.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    /// The root manifest, read as text: a crate that stops inheriting the workspace version
    /// (`version.workspace = true`) gets its own CARGO_PKG_VERSION and fails here.
    const WORKSPACE_MANIFEST: &str =
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml"));

    /// The `version = "…"` line under `[workspace.package]`.
    fn workspace_version(manifest: &str) -> Option<&str> {
        let mut in_package = false;
        for line in manifest.lines().map(str::trim) {
            if line.starts_with('[') {
                in_package = line == "[workspace.package]";
            } else if in_package && let Some(v) = line.strip_prefix("version = \"") {
                return v.strip_suffix('"');
            }
        }
        None
    }

    #[test]
    fn version_is_the_workspace_version() {
        let want = workspace_version(WORKSPACE_MANIFEST)
            .expect("Cargo.toml has a version line under [workspace.package]");
        assert_eq!(VERSION, want);
    }
}
