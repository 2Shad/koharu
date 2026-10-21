//! Foreground work tracking for long-running jobs.
//!
//! A job acquires a guard, updates it while it runs, and releases it by dropping the guard.
//! Android keeps the process alive with a foreground service and a wake lock; desktop has no
//! equivalent, so the guard is inert there.

use parking_lot::Mutex;
use tauri::{AppHandle, Manager as _};

/// Refcounted foreground work. Managed as application state.
///
/// The count and the start/stop command are one critical section: an acquire must observe the
/// effect of a concurrent release, or a `Stop` can be enqueued after the `Start` that follows it.
pub(crate) struct BackgroundWork {
    active: Mutex<usize>,
    #[cfg(target_os = "android")]
    commands: tokio::sync::mpsc::UnboundedSender<Command>,
}

impl BackgroundWork {
    pub(crate) fn new(handle: &AppHandle) -> Self {
        #[cfg(target_os = "android")]
        {
            let (commands, mut receiver) = tokio::sync::mpsc::unbounded_channel::<Command>();
            let android = handle.state::<koharu_android::Android>().inner().clone();
            drop(tauri::async_runtime::spawn(async move {
                while let Some(command) = receiver.recv().await {
                    if let Err(error) = command.run(&android).await {
                        tracing::warn!(%error, "foreground work update failed");
                    }
                }
            }));
            Self {
                active: Mutex::new(0),
                commands,
            }
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = handle;
            Self {
                active: Mutex::new(0),
            }
        }
    }
}

/// Releases the foreground work it was acquired for when dropped.
pub(crate) struct Guard {
    handle: AppHandle,
}

/// Marks foreground work as running for the lifetime of the returned guard.
pub(crate) fn acquire(handle: &AppHandle, title: &str, text: &str) -> Guard {
    let work = handle.state::<BackgroundWork>();
    let mut active = work.active.lock();
    if *active == 0 {
        work.start(title, text);
    }
    *active += 1;
    drop(active);
    Guard {
        handle: handle.clone(),
    }
}

/// Updates the notification for the work that a guard already holds.
pub(crate) fn update(handle: &AppHandle, title: &str, text: &str, done: u32, total: u32) {
    handle
        .state::<BackgroundWork>()
        .update(title, text, done, total);
}

impl Guard {
    pub(crate) fn progress(&self, title: &str, text: &str, done: u32, total: u32) {
        update(&self.handle, title, text, done, total);
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let work = self.handle.state::<BackgroundWork>();
        let mut active = work.active.lock();
        *active -= 1;
        if *active == 0 {
            work.stop();
        }
    }
}

#[cfg(target_os = "android")]
impl BackgroundWork {
    fn start(&self, title: &str, text: &str) {
        self.send(Command::Start {
            title: title.to_owned(),
            text: text.to_owned(),
        });
    }

    fn update(&self, title: &str, text: &str, done: u32, total: u32) {
        self.send(Command::Update {
            title: title.to_owned(),
            text: text.to_owned(),
            progress: Some((done, total)),
        });
    }

    fn stop(&self) {
        self.send(Command::Stop);
    }

    fn send(&self, command: Command) {
        if let Err(error) = self.commands.send(command) {
            tracing::warn!(%error, "foreground work channel is closed");
        }
    }
}

#[cfg(not(target_os = "android"))]
impl BackgroundWork {
    fn start(&self, _title: &str, _text: &str) {}

    fn update(&self, _title: &str, _text: &str, _done: u32, _total: u32) {}

    fn stop(&self) {}
}

#[cfg(target_os = "android")]
enum Command {
    Start {
        title: String,
        text: String,
    },
    Update {
        title: String,
        text: String,
        progress: Option<(u32, u32)>,
    },
    Stop,
}

#[cfg(target_os = "android")]
impl Command {
    async fn run(self, android: &koharu_android::Android) -> anyhow::Result<()> {
        match self {
            Self::Start { title, text } => android.foreground_start(&title, &text).await,
            Self::Update {
                title,
                text,
                progress,
            } => android.foreground_update(&title, &text, progress).await,
            Self::Stop => android.foreground_stop().await,
        }
    }
}
