use std::io::Read;
use std::os::unix::net::UnixStream;

use tui_test::terminal::emu::Emulator;
use tui_test::{Backend, OpenOptions};

use super::*;

fn screen(output: &[u8]) -> Frame {
    let mut emulator: Box<dyn Emulator> = Backend::Alacritty
        .build(20, 3, &OpenOptions::default().profile)
        .unwrap();
    emulator.process(output);
    Frame::capture(emulator.as_ref())
}

const STYLED: &[u8] = b"\x1b[1;31mred\x1b[0m plain \x1b[48;2;50;50;65mfold\x1b[0m\r\n\x1b[94;4m\xe2\x98\x90 box\x1b[0m \xe6\x97\xa5";

#[test]
fn a_paint_reproduces_the_screen_it_was_taken_from() {
    let original = screen(STYLED);

    let repainted = screen(&paint(&original));

    assert_eq!(repainted.grid(), original.grid());
}

#[test]
fn viewers_get_the_latest_paint_on_connecting_and_each_new_one() {
    let directory = tempfile::tempdir().unwrap();
    let stream = FrameStream::start(&directory.path().join("stream.sock")).unwrap();
    let first = screen(b"first");
    stream.send(&first);
    let mut viewer = UnixStream::connect(stream.path()).unwrap();
    let mut received = vec![0; paint(&first).len()];
    viewer.read_exact(&mut received).unwrap();
    assert_eq!(received, paint(&first));

    let second = screen(b"second");
    stream.send(&second);

    let mut received = vec![0; paint(&second).len()];
    viewer.read_exact(&mut received).unwrap();
    assert_eq!(received, paint(&second));
}

#[test]
fn a_viewer_that_never_reads_does_not_slow_publishing() {
    let directory = tempfile::tempdir().unwrap();
    let stream = FrameStream::start(&directory.path().join("stream.sock")).unwrap();
    let _stalled = UnixStream::connect(stream.path()).unwrap();
    let busy = screen(&b"x".repeat(60).repeat(3));
    let start = std::time::Instant::now();

    // Far more than a socket buffer holds.
    for _ in 0..2_000 {
        stream.send(&busy);
    }

    assert!(start.elapsed() < std::time::Duration::from_secs(2));
}

#[test]
fn a_socket_left_by_a_killed_driver_is_replaced() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("stream.sock");
    drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
    assert!(path.exists());

    let stream = FrameStream::start(&path).unwrap();

    stream.send(&screen(b"fresh"));
    assert!(UnixStream::connect(stream.path()).is_ok());
}
