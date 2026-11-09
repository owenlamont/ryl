use std::io::{IsTerminal, Write};
use std::sync::mpsc;
use std::time::Duration;

pub struct Progress {
    cancel: mpsc::SyncSender<()>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Progress {
    #[must_use]
    pub fn start(mut writer: Box<dyn Write + Send>) -> Self {
        let (cancel, receiver) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            if receiver.recv_timeout(Duration::from_millis(200))
                == Err(mpsc::RecvTimeoutError::Timeout)
            {
                let _ = writer.write_all(b"Preparing schemas...\n");
            }
        });
        Self {
            cancel,
            worker: Some(worker),
        }
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        let _ = self.cancel.try_send(());
        self.worker
            .take()
            .expect("progress worker joined once")
            .join()
            .expect("progress writer never panics");
    }
}

pub(in crate::rules::schema) fn terminal() -> Option<Progress> {
    let quiet = super::OPTIONS
        .get_or_init(super::Options::default)
        .no_progress;
    if !quiet && std::io::stderr().is_terminal() {
        Some(Progress::start(Box::new(std::io::stderr())))
    } else {
        None
    }
}
