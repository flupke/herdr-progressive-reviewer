use std::sync::Arc;
use std::thread::{self, JoinHandle};

use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
};
use tokio::sync::oneshot;

use crate::{Endpoint, Request, handler::Handler};

/// An HTTP listener whose lifetime is bounded by the reviewer's owner.
pub struct Server {
    stop: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    /// Bind before returning so the caller can report an occupied configured port.
    pub fn start(
        endpoint: Endpoint,
        dispatch: impl Fn(Request) -> Result<(), String> + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let listener = std::net::TcpListener::bind(endpoint.address())
            .map_err(|error| format!("Cannot open reviewer MCP at {}: {error}", endpoint.url()))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        let (stop, stopped) = oneshot::channel();
        let thread = thread::spawn(move || {
            runtime.block_on(async move {
                let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                    return;
                };
                let handler = Handler::new(Arc::new(dispatch));
                let mut config = StreamableHttpServerConfig::default();
                config.legacy_session_mode = false;
                config.json_response = true;
                config.allowed_origins = vec![format!("http://{}", endpoint.address())];
                let cancellation = config.cancellation_token.clone();
                let service = StreamableHttpService::new(
                    move || Ok(handler.clone()),
                    Arc::new(NeverSessionManager::default()),
                    config,
                );
                let app = axum::Router::new().nest_service("/mcp", service);
                let _ = axum::serve(listener, app)
                    .with_graceful_shutdown(async move {
                        let _ = stopped.await;
                        cancellation.cancel();
                    })
                    .await;
            });
        });
        Ok(Self {
            stop: Some(stop),
            thread: Some(thread),
        })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
