//! Documents written literally in each stored format, as earlier builds wrote them. The
//! version 4 thread document and the draft document are the exact bytes that the build
//! before the store owned the format wrote for the same content.

use std::path::Path;

use serde_json::Value;

use crate::ReviewStore;

/// The one source every fixture thread and draft shares.
pub(super) const CONTEXT: &str = r#"{"anchor":{"source_checkpoint":"checkpoint","old_path":null,"new_path":"source.rs","old_lines":null,"new_lines":{"start":0,"end":1},"target_kind":"lines","source_hunk_count":1,"old_content":null,"new_content":[102,110,32,109,97,105,110,40,41,32,123,125,10],"diff_hash":"hash"},"excerpt":"+fn main() {}"}"#;

/// The name of `CONTEXT`'s file: the SHA-256 of its JSON.
pub(super) const CONTEXT_KEY: &str =
    "59fbc32f72017e97f5c2d60cd4e07386b448432d40d1b6389756720dc2d18252";

/// A resolved, answered and read thread, and a thread waiting for an answer.
pub(super) const THREADS_V4: &str = r#"{"version":4,"conversations":{"review_unit":"change","threads":[{"id":"0b6f2a4e-1c3d-4e5f-8a9b-0c1d2e3f4a5b","context":"59fbc32f72017e97f5c2d60cd4e07386b448432d40d1b6389756720dc2d18252","messages":[{"id":"11111111-1111-4111-8111-111111111111","author":"reviewer","text":"Why \"this\"?\nExplain.","sequence":1},{"id":"22222222-2222-4222-8222-222222222222","author":"agent","text":"Because.","in_reply_to":"11111111-1111-4111-8111-111111111111","sequence":2}],"resolution":"resolved","seen_reply_through":2,"seen_replies":["22222222-2222-4222-8222-222222222222"]},{"id":"3c4d5e6f-7a8b-4c9d-8e0f-1a2b3c4d5e6f","context":"59fbc32f72017e97f5c2d60cd4e07386b448432d40d1b6389756720dc2d18252","messages":[{"id":"33333333-3333-4333-8333-333333333333","author":"reviewer","text":"Pending","sequence":3}],"resolution":"open","seen_reply_through":0,"seen_replies":[]}],"sequence":3,"answered":{"0b6f2a4e-1c3d-4e5f-8a9b-0c1d2e3f4a5b":1,"3c4d5e6f-7a8b-4c9d-8e0f-1a2b3c4d5e6f":0}}}"#;

/// A reply to the waiting thread of `THREADS_V4` and a draft for a new thread.
pub(super) const DRAFTS_V1: &str = r#"{"version":1,"review_unit":"change","drafts":[{"target":{"Thread":"3c4d5e6f-7a8b-4c9d-8e0f-1a2b3c4d5e6f"},"source":{"context":"59fbc32f72017e97f5c2d60cd4e07386b448432d40d1b6389756720dc2d18252"},"reply_to":"33333333-3333-4333-8333-333333333333","text":"Unposted \"reply\"\n","id":"44444444-4444-4444-8444-444444444444","thread":"3c4d5e6f-7a8b-4c9d-8e0f-1a2b3c4d5e6f"},{"target":{"File":"source.rs"},"source":{"context":"59fbc32f72017e97f5c2d60cd4e07386b448432d40d1b6389756720dc2d18252"},"reply_to":null,"text":"New thread","id":"55555555-5555-4555-8555-555555555555","thread":"66666666-6666-4666-8666-666666666666"}]}"#;

/// `CONTEXT` as a JSON object, to merge into the threads of documents that carry their
/// sources inline.
pub(super) fn inline_source() -> Value {
    serde_json::from_str(CONTEXT).unwrap()
}

/// A thread row that carries `CONTEXT` inline, as versions 2 and 3 stored it.
pub(super) fn inline_thread(mut row: Value) -> Value {
    let Value::Object(source) = inline_source() else {
        unreachable!()
    };
    row.as_object_mut().unwrap().extend(source);
    row
}

impl ReviewStore {
    /// Write `CONTEXT`'s file as earlier builds did.
    pub(super) fn write_context_fixture(&self) {
        self.write_json_fixture(&self.thread_source_path(CONTEXT_KEY).unwrap(), CONTEXT);
    }

    pub(super) fn write_json_fixture(&self, path: &Path, json: &str) {
        self.atomic_compressed_bytes(path, json.as_bytes(), "write fixture")
            .unwrap();
    }

    pub(super) fn threads_fixture_path(&self) -> std::path::PathBuf {
        self.review_record_path("conversations", &"change".into())
            .unwrap()
    }

    pub(super) fn decoded_fixture(path: &Path) -> String {
        String::from_utf8(
            ReviewStore::decode_thread_json(path, &std::fs::read(path).unwrap()).unwrap(),
        )
        .unwrap()
    }
}
