//! platform/procargs ctx: pure decoder for the macOS `KERN_PROCARGS2` buffer.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  src/platform/procargs.rs
//! Deps:    none — pure over bytes (compiled on every target so fixtures always run)
//! Tested:  inline `#[cfg(test)]` — synthetic buffers per the layout below
//!
//! Key responsibilities:
//! - Split a raw procargs2 buffer into `(argv, env_region)`, argc-bounded — an argv string
//!   that LOOKS like `WEZTERM_PANE=…` must never be misread as environment.
//!
//! Buffer layout (xnu `sysctl_procargsx`):
//!   `argc: i32 (native endian)` · exec path (NUL-terminated) · NUL padding · argc argv
//!   strings (NUL-terminated) · env strings (NUL-terminated) · trailing garbage possible.
//!
//! Design constraints:
//! - Tolerant: malformed input → `None`, never a panic; truncated env degrades, argv intact.

// Consumed by the macOS provider only; stays compiled everywhere so the fixtures run on
// every dev machine.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

/// Decoded argv + the raw env region (NUL-separated, ready for `discovery::parse_environ`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcArgs {
    /// Exactly `argc` argv strings.
    pub argv: Vec<Vec<u8>>,
    /// Everything after the last argv string — the environment region. Empty when the kernel
    /// omitted it (`cs_restricted`) or the buffer was truncated at the boundary.
    pub env_region: Vec<u8>,
}

/// Decode a raw `KERN_PROCARGS2` buffer. `None` = malformed (short header, bad argc, argv
/// walks off the end).
pub fn decode(buf: &[u8]) -> Option<ProcArgs> {
    let header: [u8; 4] = buf.get(..4)?.try_into().ok()?;
    let arg_count = i32::from_ne_bytes(header);
    if arg_count < 0 {
        return None;
    }
    let rest = &buf[4..];
    // Skip the exec path…
    let mut pos = rest.iter().position(|&b| b == 0)?;
    // …and the NUL padding run after it.
    while rest.get(pos) == Some(&0) {
        pos += 1;
    }
    // argc NUL-terminated argv strings.
    let mut argv = Vec::with_capacity(usize::try_from(arg_count).ok()?);
    for _ in 0..arg_count {
        let start = pos;
        let len = rest.get(start..)?.iter().position(|&b| b == 0)?;
        argv.push(rest[start..start + len].to_vec());
        pos = start + len + 1;
    }
    let env_region = rest.get(pos..).unwrap_or_default().to_vec();
    Some(ProcArgs { argv, env_region })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(argc: i32, tail: &[u8]) -> Vec<u8> {
        let mut b = argc.to_ne_bytes().to_vec();
        b.extend_from_slice(tail);
        b
    }

    #[test]
    fn normal_buffer_splits_argv_and_env() {
        let b = buf(2, b"/usr/bin/codex\0\0\0codex\0--x\0HOME=/u\0TERM=xterm\0");
        let p = decode(&b).expect("well-formed");
        assert_eq!(p.argv, vec![b"codex".to_vec(), b"--x".to_vec()]);
        assert_eq!(p.env_region, b"HOME=/u\0TERM=xterm\0");
    }

    #[test]
    fn argv_that_looks_like_env_stays_argv() {
        let b = buf(2, b"/bin/x\0\0x\0WEZTERM_PANE=9\0CLAUDE_ACCOUNT=real\0");
        let p = decode(&b).expect("well-formed");
        assert_eq!(
            p.argv[1], b"WEZTERM_PANE=9",
            "argc bounds argv — assignment-shaped args never leak into env"
        );
        assert_eq!(p.env_region, b"CLAUDE_ACCOUNT=real\0");
    }

    #[test]
    fn env_omitted_is_empty_region_argv_intact() {
        // cs_restricted shape: argv present, nothing after.
        let b = buf(1, b"/bin/x\0\0\0x\0");
        let p = decode(&b).expect("well-formed");
        assert_eq!(p.argv, vec![b"x".to_vec()]);
        assert!(p.env_region.is_empty());
    }

    #[test]
    fn empty_argv_entries_survive() {
        let b = buf(3, b"/bin/x\0\0a\0\0b\0E=1\0");
        let p = decode(&b).expect("well-formed");
        assert_eq!(p.argv, vec![b"a".to_vec(), Vec::new(), b"b".to_vec()]);
        assert_eq!(p.env_region, b"E=1\0");
    }

    #[test]
    fn argc_zero_yields_no_argv_all_env() {
        let b = buf(0, b"/bin/x\0\0E=1\0");
        let p = decode(&b).expect("well-formed");
        assert!(p.argv.is_empty());
        assert_eq!(p.env_region, b"E=1\0");
    }

    #[test]
    fn malformed_buffers_are_none_never_panic() {
        assert_eq!(decode(b""), None, "short header");
        assert_eq!(decode(&buf(-1, b"/x\0")), None, "negative argc");
        assert_eq!(
            decode(&buf(3, b"/bin/x\0\0only-one\0")),
            None,
            "argv truncated"
        );
        assert_eq!(
            decode(&buf(1, b"no-nul-anywhere")),
            None,
            "unterminated exec path"
        );
    }
}
