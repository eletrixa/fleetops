//! Path resolution — cwd-independent, env-overridable for dev.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/paths.rs
//! Deps:    std only
//! Tested:  inline `#[cfg(test)]` (default shape; the env override is a one-liner read at runtime)
//!
//! Key responsibilities:
//! - Resolve the unified Claude dir (`~/.claude` — all accounts symlink into it, recon D2).
//!
//! Design constraints:
//! - `FLEET_CLAUDE_DIR` overrides for dev/testing against a fixture tree.

use std::path::PathBuf;

/// The unified Claude state dir: `$FLEET_CLAUDE_DIR` override, else `$HOME/.claude`.
pub fn claude_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("FLEET_CLAUDE_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
    home.join(".claude")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_home_claude() {
        // Serial-safety: don't set the override in tests (env is process-global);
        // just assert the default shape.
        if std::env::var_os("FLEET_CLAUDE_DIR").is_none() {
            assert!(claude_dir().ends_with(".claude"));
        }
    }
}
