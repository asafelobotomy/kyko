#![forbid(unsafe_code)]

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
