//! fleetops-procargs — the ONE unsafe syscall fleetops needs on macOS, isolated.
//!
//! Project: Fleetops — TUI monitoring all running Claude Code sessions (the fleet)
//! Module:  fleetops-procargs/src/lib.rs
//! Deps:    libc (sysctl, KERN_PROCARGS2)
//! Tested:  self-probe test against the test process's own pid (macOS only)
//!
//! Key responsibilities:
//! - Expose `procargs2(pid) -> io::Result<Vec<u8>>`: the raw KERN_PROCARGS2 buffer for a pid.
//!   Decoding (argc parse, argv/env split) lives in the main crate as pure, fixture-tested code.
//!
//! Design constraints:
//! - The main crate keeps `unsafe_code = "forbid"`; every unsafe byte lives here, ~40 lines.
//! - Buffer is allocated at KERN_ARGMAX — truncation is the kernel's, never this crate's.

#![deny(unsafe_op_in_unsafe_fn)]
#![cfg(target_os = "macos")]

use std::io;

/// Kernel argument-space upper bound (`sysctl kern.argmax`) — the KERN_PROCARGS2 buffer size.
fn argmax() -> io::Result<usize> {
    let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
    let mut value: libc::c_int = 0;
    let mut size = std::mem::size_of::<libc::c_int>();
    // SAFETY: mib/value/size are valid for the call's duration; the kernel writes at most
    // `size` bytes into `value`, which is exactly sized for it.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as libc::c_uint,
            std::ptr::from_mut(&mut value).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(value as usize)
}

/// Raw `KERN_PROCARGS2` buffer for `pid`: `argc: c_int`, exec path, NUL padding, argv strings,
/// then environment strings — all NUL-separated. Errors map straight from the kernel
/// (`EPERM`/`ESRCH`/…); the caller distinguishes denied vs gone via `raw_os_error`.
pub fn procargs2(pid: u32) -> io::Result<Vec<u8>> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as libc::c_int];
    let mut size = argmax()?;
    let mut buf = vec![0u8; size];
    // SAFETY: `buf` is `size` bytes and outlives the call; the kernel writes at most `size`
    // bytes and updates `size` to the written length.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as libc::c_uint,
            buf.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    buf.truncate(size);
    Ok(buf)
}

// --- fd1 pty path: proc_pidfdinfo(PROC_PIDFDVNODEPATHINFO) ---------------------------------
//
// libproc-the-crate keeps its raw bindings private and wraps no vnode-path flavor, so the one
// extra syscall lives here beside the sysctl. Layout per xnu's public <sys/proc_info.h> (ABI
// stable): struct vnode_fdinfowithpath = proc_fileinfo (24 bytes) + vnode_info (208 bytes) +
// char vip_path[MAXPATHLEN=1024].

// Sizes verified live against this kernel (probe: written=1200, "/dev/null" at offset 176):
// proc_fileinfo (24) + vnode_info (152 = vinfo_stat 136 + vi_type 4 + vi_pad 4 + fsid 8) +
// vip_path[MAXPATHLEN=1024].
const PROC_PIDFDVNODEPATHINFO: libc::c_int = 2;
const VNODE_FDINFOWITHPATH_SIZE: usize = 24 + 152 + 1024;
const VIP_PATH_OFFSET: usize = 24 + 152;

unsafe extern "C" {
    fn proc_pidfdinfo(
        pid: libc::c_int,
        fd: libc::c_int,
        flavor: libc::c_int,
        buffer: *mut libc::c_void,
        buffersize: libc::c_int,
    ) -> libc::c_int;
}

/// Resolved filesystem path of one fd of `pid` (fleetops asks about fd 1), when the fd is a
/// vnode (files, ptys). `Ok(None)` = fd exists but is not a vnode (socket, pipe) or is closed;
/// `Err` = the kernel refused (EPERM etc.).
pub fn fd_path(pid: u32, fd: i32) -> io::Result<Option<String>> {
    let mut buf = vec![0u8; VNODE_FDINFOWITHPATH_SIZE];
    // SAFETY: `buf` is exactly VNODE_FDINFOWITHPATH_SIZE bytes and outlives the call; the
    // kernel writes at most `buffersize` bytes into it.
    let written = unsafe {
        proc_pidfdinfo(
            pid as libc::c_int,
            fd,
            PROC_PIDFDVNODEPATHINFO,
            buf.as_mut_ptr().cast(),
            VNODE_FDINFOWITHPATH_SIZE as libc::c_int,
        )
    };
    if written <= 0 {
        let err = io::Error::last_os_error();
        // EBADF / not-a-vnode surfaces as a failed call with EBADF — that's "no pty", not an
        // acquisition failure worth a Denied.
        return match err.raw_os_error() {
            Some(libc::EBADF) => Ok(None),
            _ => Err(err),
        };
    }
    if (written as usize) < VNODE_FDINFOWITHPATH_SIZE {
        return Ok(None); // short struct = a different fd flavor; not a vnode path
    }
    let path_bytes = &buf[VIP_PATH_OFFSET..];
    let end = path_bytes.iter().position(|&b| b == 0).unwrap_or(0);
    if end == 0 {
        return Ok(None);
    }
    Ok(std::str::from_utf8(&path_bytes[..end])
        .ok()
        .map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_process_argv_is_readable() {
        let buf = procargs2(std::process::id()).expect("own pid always readable");
        assert!(buf.len() > 4, "argc header + exec path at minimum");
        let argc = i32::from_ne_bytes(buf[..4].try_into().unwrap());
        assert!(argc >= 1, "test binary has at least argv0, got {argc}");
    }

    #[test]
    fn dead_pid_errors() {
        // pid 1 is launchd (EPERM for non-root) or an unreachable pid errors — either way Err.
        assert!(procargs2(u32::MAX - 1).is_err());
    }

    #[test]
    fn own_fd_paths_resolve() {
        use std::os::fd::AsRawFd;
        // The test harness may pipe stdout; open a real file so the probed fd is a vnode.
        let f = std::fs::File::open("/dev/null").unwrap();
        let path = fd_path(std::process::id(), f.as_raw_fd())
            .expect("own fds readable")
            .expect("an open file is a vnode");
        assert_eq!(path, "/dev/null");
    }
}
