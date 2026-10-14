//! Kill-on-close job: the kernel kills the server whenever Koharu exits,
//! including crashes that skip `kill_on_drop`.

use anyhow::{Context, Result};
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    },
};

pub(crate) struct KillOnClose(HANDLE);

// Job handles are usable and closable from any thread.
unsafe impl Send for KillOnClose {}

impl KillOnClose {
    pub(crate) fn assign(child: &tokio::process::Child) -> Result<Self> {
        let process = HANDLE(
            child
                .raw_handle()
                .context("speech server has no process handle")?,
        );
        // SAFETY: the job handle is owned by `Self` and closed exactly once in
        // `Drop`; the process handle is borrowed from a live `Child`.
        unsafe {
            let job = Self(CreateJobObjectW(None, None).context("failed to create a job object")?);
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .context("failed to configure the speech server job")?;
            AssignProcessToJobObject(job.0, process)
                .context("failed to bind the speech server to Koharu's lifetime")?;
            Ok(job)
        }
    }
}

impl Drop for KillOnClose {
    fn drop(&mut self) {
        // SAFETY: `self.0` came from `CreateJobObjectW` and is closed only here.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
