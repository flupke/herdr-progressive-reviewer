use lsp_server::Notification;

use super::*;

#[test]
fn expert_preamble_is_skipped_without_losing_protocol_messages() {
    let mut bytes = b"[i] New install path is: /tmp/expert\n\n".to_vec();
    let message = Message::Notification(Notification::new("ready".to_owned(), ()));
    message.write(&mut bytes).unwrap();
    message.write(&mut bytes).unwrap();
    let mut reader = MessageReader::new(Cursor::new(bytes), LanguageServer::Expert);
    for _ in 0..2 {
        assert!(
            matches!(reader.read().unwrap(), Some(Message::Notification(notification)) if notification.method == "ready")
        );
    }
    assert!(reader.read().unwrap().is_none());
}

#[test]
fn malformed_protocol_is_rejected_after_the_expert_preamble() {
    let message = Message::Notification(Notification::new("ready".to_owned(), ()));
    let mut bytes = Vec::new();
    message.write(&mut bytes).unwrap();
    bytes.extend_from_slice(b"not a protocol header\n");
    let mut reader = MessageReader::new(Cursor::new(bytes), LanguageServer::Expert);
    assert!(
        matches!(reader.read().unwrap(), Some(Message::Notification(notification)) if notification.method == "ready")
    );
    assert!(reader.read().is_err());
}

#[test]
fn other_servers_remain_strict_and_expert_preamble_is_bounded() {
    let mut strict = MessageReader::new(Cursor::new(b"startup\n"), LanguageServer::TypeScript);
    assert!(strict.read().is_err());
    let mut expert = MessageReader::new(Cursor::new(vec![b'x'; 17 * 1024]), LanguageServer::Expert);
    assert!(expert.read().is_err());
}
