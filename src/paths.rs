//! Path resolution — cwd-independent, env-overridable for dev.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/paths.rs
//! Deps:    std only
//! Tested:  inline `#[cfg(test)]` (default shape; the env override is a one-liner read at runtime)
//!
//! Key responsibilities:
//! - Resolve the unified Claude dir (`~/.claude` — all accounts symlink into it, recon D2).
//! - Resolve the Codex CLI state dir (`~/.codex`, spec 008) — same override/default shape.
//!
//! Design constraints:
//! - `FLEET_CLAUDE_DIR` / `FLEET_CODEX_DIR` override for dev/testing against a fixture tree.

use std::path::PathBuf;

/// The unified Claude state dir: `$FLEET_CLAUDE_DIR` override, else `$HOME/.claude`.
pub fn claude_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("FLEET_CLAUDE_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
    home.join(".claude")
}

/// The Codex CLI state dir: `$FLEET_CODEX_DIR` override, else `$HOME/.codex` (spec 008;
/// mirrors `claude_dir()`'s override/default convention exactly).
pub fn codex_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("FLEET_CODEX_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
    home.join(".codex")
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

    #[test]
    fn default_is_home_codex() {
        if std::env::var_os("FLEET_CODEX_DIR").is_none() {
            assert!(codex_dir().ends_with(".codex"));
        }
    }
}
