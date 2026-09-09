use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::sync::mpsc::{self, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use component_core::EventEnvelope;

#[derive(Default)]
pub(super) struct Recorder {
    samples: Option<SyncSender<Sample>>,
    writer: Option<JoinHandle<()>>,
}

struct Sample {
    name: &'static str,
    queued: Duration,
    work: Duration,
}

impl Recorder {
    pub(super) fn from_env() -> std::io::Result<Self> {
        let Some(path) = std::env::var_os("HERDR_REVIEWER_TIMINGS") else {
            return Ok(Self::default());
        };
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let (samples, receiver) = mpsc::sync_channel::<Sample>(1024);
        let writer = thread::spawn(move || {
            let mut output = BufWriter::new(file);
            while let Ok(sample) = receiver.recv() {
                let line = serde_json::json!({
                    "event": sample.name,
                    "queued_ms": sample.queued.as_secs_f64() * 1000.0,
                    "work_ms": sample.work.as_secs_f64() * 1000.0,
                });
                if writeln!(output, "{line}")
                    .and_then(|()| output.flush())
                    .is_err()
                {
                    break;
                }
            }
        });
        Ok(Self {
            samples: Some(samples),
            writer: Some(writer),
        })
    }

    pub(super) fn event(&self, event: &EventEnvelope, started: Instant) {
        self.record(Sample {
            name: event.type_name(),
            queued: started.saturating_duration_since(event.created_at()),
            work: started.elapsed(),
        });
    }

    pub(super) fn frame(&self, started: Instant) {
        self.record(Sample {
            name: "frame",
            queued: Duration::ZERO,
            work: started.elapsed(),
        });
    }

    fn record(&self, sample: Sample) {
        if let Some(samples) = &self.samples {
            // Diagnostics must never wait for the filesystem or a full queue.
            let _ = samples.try_send(sample);
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.samples.take();
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}
