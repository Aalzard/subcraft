//! Background `.spp` open so the UI stays responsive.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::spp::SppOpenResult;
use crate::Session;

const OPEN_TIMEOUT: Duration = Duration::from_secs(180);

pub struct SppOpenJob {
    path: PathBuf,
    rx: mpsc::Receiver<Result<SppOpenResult, String>>,
    started: Instant,
    timed_out: bool,
}

impl SppOpenJob {
    pub fn start(path: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        let work = path.clone();
        std::thread::spawn(move || {
            let r = Session::open_from_spp(&work);
            let _ = tx.send(r);
        });
        Self {
            path,
            rx,
            started: Instant::now(),
            timed_out: false,
        }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// `Some` when the worker finished (success or error), or after [`OPEN_TIMEOUT`].
    pub fn poll(&mut self) -> Option<Result<SppOpenResult, String>> {
        if let Ok(r) = self.rx.try_recv() {
            return Some(r);
        }
        if !self.timed_out && self.started.elapsed() > OPEN_TIMEOUT {
            self.timed_out = true;
            let log = subcraft_spp::open_log_path();
            return Some(Err(format!(
                "SPP open timed out after {} minutes. Copy the .spp to a local folder (e.g. Desktop) or read progress in {}",
                OPEN_TIMEOUT.as_secs() / 60,
                log.display()
            )));
        }
        None
    }
}
