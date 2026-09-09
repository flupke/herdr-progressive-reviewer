use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use syntax_highlighting::SyntaxHighlighter;
use ui_events::{HighlightRequest, HighlightingFinished};

pub(super) struct Worker {
    commands: Sender<Option<HighlightRequest>>,
    thread: Option<JoinHandle<()>>,
    stopped: Arc<AtomicBool>,
}

impl Worker {
    pub(super) fn start(
        syntax: SyntaxHighlighter,
        deliver: impl Fn(HighlightingFinished) + Send + 'static,
    ) -> Self {
        let (commands, receiver) = mpsc::channel();
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stopped = Arc::clone(&stopped);
        let thread = thread::spawn(move || {
            let cancelled = || worker_stopped.load(Ordering::Relaxed);
            while let Ok(Some(request)) = receiver.recv() {
                if cancelled() {
                    break;
                }
                let highlighted = match &request {
                    HighlightRequest::Diff(content) => syntax.highlight_unless(
                        &content.path,
                        content.rows.clone(),
                        content.old_content.as_deref(),
                        content.new_content.as_deref(),
                        cancelled,
                    ),
                    HighlightRequest::Source(content) => syntax.highlight_unless(
                        &content.location.path.to_string_lossy(),
                        Vec::new(),
                        None,
                        Some(&content.content),
                        cancelled,
                    ),
                };
                if let Some(highlighted) = highlighted {
                    deliver(HighlightingFinished {
                        request,
                        highlighted,
                    });
                }
            }
        });
        Self {
            commands,
            thread: Some(thread),
            stopped,
        }
    }

    pub(super) fn submit(&self, request: HighlightRequest) -> Result<(), String> {
        self.commands
            .send(Some(request))
            .map_err(|_| "syntax highlighting worker stopped".to_owned())
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        let _ = self.commands.send(None);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
