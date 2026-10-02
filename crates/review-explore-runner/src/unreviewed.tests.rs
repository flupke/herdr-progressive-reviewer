use super::*;

fn written(files: usize) -> Unreviewed {
    Unreviewed {
        directory: "/tmp/unreviewed".into(),
        files,
        ..Unreviewed::default()
    }
}

#[test]
fn the_prompt_names_the_directory_of_the_unreviewed_diffs() {
    assert_eq!(
        written(5).to_string(),
        "\nUnreviewed diffs: /tmp/unreviewed/<repository path>, for the 5 files with unreviewed lines\n"
    );
    assert_eq!(
        written(1).to_string(),
        "\nUnreviewed diffs: /tmp/unreviewed/<repository path>, for the 1 file with unreviewed lines\n"
    );
}

#[test]
fn a_fully_reviewed_checkpoint_says_so() {
    assert_eq!(
        written(0).to_string(),
        "\nUnreviewed diffs: none; every changed line is reviewed.\n"
    );
}

#[test]
fn displaced_diffs_and_a_notice_are_stated_only_when_there_are_some() {
    let unreviewed = Unreviewed {
        index: Some("/tmp/unreviewed/__herdr_reviewer_index__".into()),
        notice: Some("the code changed since this round started".into()),
        ..written(2)
    };

    assert_eq!(
        unreviewed.to_string(),
        "\nNote: the code changed since this round started\n\n\
         Unreviewed diffs: /tmp/unreviewed/<repository path>, for the 2 files with unreviewed lines\n\
         Displaced diffs: /tmp/unreviewed/__herdr_reviewer_index__ says where the diffs that are \
         not at their repository path are\n"
    );
}
