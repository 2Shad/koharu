//! `PR_SET_PDEATHSIG`: the kernel kills the server when Koharu exits,
//! including exits that skip `kill_on_drop`.

pub(crate) fn kill_with_parent(command: &mut tokio::process::Command) {
    // SAFETY: the hook runs between fork and exec and only calls `prctl`,
    // which is async-signal-safe.
    unsafe {
        command.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}
