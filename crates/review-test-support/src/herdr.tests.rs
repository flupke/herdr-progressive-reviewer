use super::*;
use crate::eventually;

#[test]
fn the_server_stops_when_its_owner_is_killed() {
    let repository = tempfile::tempdir().unwrap();
    let mut server = HerdrTestServer::start(repository.path());
    let child = server.child.as_mut().unwrap();
    // Hand the only writing end of the server's pipe to a stand-in owner.
    let mut owner = Command::new("sleep")
        .arg("60")
        .stdout(Stdio::from(child.stdin.take().unwrap()))
        .spawn()
        .unwrap();

    owner.kill().unwrap();
    owner.wait().unwrap();

    assert!(eventually(Duration::from_secs(5), || child
        .try_wait()
        .unwrap()
        .is_some()));
    assert!(UnixStream::connect(&server.socket_path).is_err());
}

#[test]
fn a_dropped_server_stops_answering() {
    let repository = tempfile::tempdir().unwrap();
    let server = HerdrTestServer::start(repository.path());
    let socket_path = server.socket_path.clone();

    drop(server);

    assert!(UnixStream::connect(socket_path).is_err());
}
