use super::*;

fn project() -> (tempfile::TempDir, Transcripts, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("-work-repository");
    fs::create_dir_all(&directory).unwrap();
    let transcripts = Transcripts::at(root.path().to_owned());
    (root, transcripts, directory)
}

#[test]
fn the_last_entry_is_the_last_conversation_line_and_the_model_the_last_answer_s() {
    let (_root, transcripts, directory) = project();
    fs::write(
        directory.join("s1.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","message":{"role":"user"}}"#,
            "\n",
            r#"{"type":"assistant","uuid":"a1","message":{"model":"claude-sonnet-5-5"}}"#,
            "\n",
            r#"{"type":"assistant","uuid":"a2","message":{"model":"<synthetic>"}}"#,
            "\n",
            r#"{"type":"last-prompt","uuid":"x"}"#,
            "\n",
        ),
    )
    .unwrap();

    assert_eq!(
        transcripts.last_entry("s1"),
        Some(LastEntry {
            entry: Some("a2".into()),
            model: Some("claude-sonnet-5-5".into()),
        })
    );
    assert_eq!(transcripts.last_entry("s2"), None);
}

#[test]
fn deleting_a_session_removes_its_transcript_and_its_side_directory_only() {
    let (_root, transcripts, directory) = project();
    fs::write(directory.join("fork.jsonl"), "{}\n").unwrap();
    fs::create_dir_all(directory.join("fork/tool-results")).unwrap();
    fs::write(directory.join("parent.jsonl"), "{}\n").unwrap();

    transcripts.delete("fork");

    assert!(transcripts.find("fork").is_none());
    assert!(!directory.join("fork").exists());
    assert!(transcripts.find("parent").is_some());
}
