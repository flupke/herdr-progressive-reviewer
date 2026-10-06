use super::*;

const DIFFS_DIRECTORY: &str = "/tmp/unreviewed/<repository path>";

fn written(files: usize) -> Unreviewed {
    Unreviewed {
        directory: "/tmp/unreviewed".into(),
        files,
        ..Unreviewed::default()
    }
}

#[test]
fn the_prompt_names_the_directory_of_the_unreviewed_diffs() {
    let many = written(5).to_string();
    assert!(many.contains(DIFFS_DIRECTORY), "{many}");
    assert!(many.contains("5 files"), "{many}");

    let one = written(1).to_string();
    assert!(one.contains(DIFFS_DIRECTORY), "{one}");
    assert!(one.contains("1 file") && !one.contains("1 files"), "{one}");
}

#[test]
fn a_fully_reviewed_checkpoint_names_no_directory() {
    let none = written(0).to_string();

    assert!(none.contains("none"), "{none}");
    assert!(!none.contains("/tmp/unreviewed"), "{none}");
    assert!(!none.contains(" 0 "), "{none}");
}

#[test]
fn displaced_diffs_and_a_notice_are_stated_only_when_there_are_some() {
    let index = "/tmp/unreviewed/__herdr_reviewer_index__";
    let notice = "the code changed since this round started";
    let unreviewed = Unreviewed {
        index: Some(index.into()),
        notice: Some(notice.into()),
        ..written(2)
    }
    .to_string();

    let notice_at = unreviewed.find(notice).unwrap();
    let directory_at = unreviewed.find(DIFFS_DIRECTORY).unwrap();
    let index_at = unreviewed.find(index).unwrap();
    assert!(
        notice_at < directory_at && directory_at < index_at,
        "{unreviewed}"
    );

    let without_extras = written(2).to_string();
    assert!(!without_extras.contains(index), "{without_extras}");
    assert!(!without_extras.contains(notice), "{without_extras}");
}
