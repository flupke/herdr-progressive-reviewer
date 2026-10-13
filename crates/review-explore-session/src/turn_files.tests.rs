use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;

use review_explore::{Comparison, Exploration};
use review_explore_runner::{INDEX, RoundData, TurnInstructions, Unreviewed};

use super::TurnFiles;

fn turn() -> TurnInstructions {
    let comparison = Comparison {
        repository_root: "/repository".into(),
        checkpoint: serde_json::from_value(serde_json::json!({"review_unit":"r","checkpoint":"c"}))
            .unwrap(),
        files: Vec::new(),
        context: Vec::new(),
        diffs: Vec::new(),
        manifest: Vec::new(),
        sources: Vec::new(),
        base: None,
    };
    let request = Exploration::new(Arc::new(comparison.clone()))
        .request(None, None)
        .unwrap();
    TurnInstructions::prepare(
        &request,
        &comparison,
        "the-access",
        &Unreviewed::default(),
        &RoundData::default(),
        None,
    )
}

/// The directory of the index the prompt names.
fn directory(prompt: &str) -> PathBuf {
    let index = prompt
        .split_whitespace()
        .find(|word| word.ends_with(INDEX))
        .expect(prompt);
    PathBuf::from(index).parent().unwrap().to_owned()
}

#[test]
fn the_prompt_names_the_index_of_a_private_directory_that_holds_every_file_of_the_turn() {
    let turn = turn();

    let files = TurnFiles::write(&turn).unwrap();

    let directory = directory(files.prompt());
    let mode = std::fs::metadata(&directory).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o700, "{mode:o}");
    for file in turn.files() {
        assert_eq!(
            std::fs::read_to_string(directory.join(file.name)).unwrap(),
            file.text,
            "{}",
            file.name
        );
    }
    drop(files);
    assert!(!directory.exists(), "the files go with the turn");
}

#[test]
fn each_turn_gets_a_directory_of_its_own() {
    let turn = turn();

    let first = TurnFiles::write(&turn).unwrap();
    let second = TurnFiles::write(&turn).unwrap();

    assert_ne!(directory(first.prompt()), directory(second.prompt()));
}
