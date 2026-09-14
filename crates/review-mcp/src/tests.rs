use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use rmcp::{ServiceExt, model::ClientInfo, transport::StreamableHttpClientTransport};

use super::{Endpoint, Response, Server};

struct ServerFixture {
    endpoint: Endpoint,
    _repository: tempfile::TempDir,
}

impl ServerFixture {
    fn new() -> Self {
        let repository = tempfile::tempdir().unwrap();
        let endpoint = Endpoint::for_repository(repository.path(), None).unwrap();
        assert_eq!(
            endpoint,
            Endpoint::for_repository(&repository.path().join("."), None).unwrap()
        );
        assert!(Endpoint::for_repository(repository.path(), Some(0)).is_err());
        Self {
            endpoint,
            _repository: repository,
        }
    }

    fn start(&self) -> Server {
        Server::start(self.endpoint, |request| {
            request.respond(Ok(Response::Threads(Vec::new())));
            Ok(())
        })
        .unwrap()
    }

    fn rejected_request(&self, host: &str, origin: &str) {
        let mut stream = TcpStream::connect(self.endpoint.address()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        write!(stream, "GET /mcp HTTP/1.1\r\nHost: {host}\r\nOrigin: {origin}\r\nAccept: text/event-stream\r\nConnection: close\r\n\r\n").unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 403"), "{response}");
    }
}

#[test]
fn a_closed_reviewer_is_unavailable_and_a_new_client_can_reconnect_after_reopening() {
    let fixture = ServerFixture::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        assert!(
            ClientInfo::default()
                .serve(StreamableHttpClientTransport::from_uri(
                    fixture.endpoint.url()
                ))
                .await
                .is_err()
        );
        for _ in 0..2 {
            let server = fixture.start();
            assert!(Server::start(fixture.endpoint, |_| Ok(())).is_err());
            let client = ClientInfo::default()
                .serve(StreamableHttpClientTransport::from_uri(
                    fixture.endpoint.url(),
                ))
                .await
                .unwrap();
            assert_eq!(client.list_all_tools().await.unwrap().len(), 4);
            client.cancel().await.unwrap();
            drop(server);
            assert!(TcpStream::connect(fixture.endpoint.address()).is_err());
        }
    });
}

#[test]
fn the_sdk_rejects_foreign_origins_and_hosts() {
    let fixture = ServerFixture::new();
    let _server = fixture.start();
    fixture.rejected_request(
        &fixture.endpoint.address().to_string(),
        "https://foreign.example",
    );
    fixture.rejected_request(
        "foreign.example",
        &format!("http://{}", fixture.endpoint.address()),
    );
}
