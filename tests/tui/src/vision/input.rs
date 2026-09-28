use std::io::{self, BufRead};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use anyhow::Result;
use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::{Handle, Signals};

pub(super) struct Input {
    pub(super) events: Receiver<Option<io::Result<String>>>,
    signals: Handle,
}

impl Input {
    pub(super) fn start() -> Result<Self> {
        let (send, events) = mpsc::channel();
        let mut signals = Signals::new([SIGHUP, SIGINT, SIGTERM])?;
        let handle = signals.handle();
        let stop = send.clone();
        thread::spawn(move || {
            if signals.forever().next().is_some() {
                let _ = stop.send(None);
            }
        });
        thread::spawn(move || {
            for line in io::stdin().lock().lines() {
                if send.send(Some(line)).is_err() {
                    return;
                }
            }
            let _ = send.send(None);
        });
        Ok(Self {
            events,
            signals: handle,
        })
    }
}

impl Drop for Input {
    fn drop(&mut self) {
        self.signals.close();
        // Both input readers belong to this driver process. Do not block its
        // exit if a sandbox denies the signal iterator's socket wakeup.
    }
}
