use super::*;

#[test]
fn the_next_round_is_written_in_simplified_technical_english_by_default() {
    assert_eq!(
        ExploreRoundSettings::default().writing,
        WritingStyle::SimplifiedTechnicalEnglish
    );
}

#[test]
fn settings_saved_without_a_value_take_its_default() {
    let empty: ExploreRoundSettings = serde_json::from_str("{}").unwrap();

    assert_eq!(empty, ExploreRoundSettings::default());
}

#[test]
fn settings_read_back_as_they_were_saved() {
    for writing in [
        WritingStyle::Plain,
        WritingStyle::SimplifiedTechnicalEnglish,
    ] {
        let settings = ExploreRoundSettings { writing };

        let saved = serde_json::to_string(&settings).unwrap();

        assert_eq!(
            serde_json::from_str::<ExploreRoundSettings>(&saved).unwrap(),
            settings
        );
    }
}

#[test]
fn turning_the_writing_style_over_gives_the_other_style() {
    let mut writing = WritingStyle::SimplifiedTechnicalEnglish;

    writing.toggle();
    assert_eq!(writing, WritingStyle::Plain);
    writing.toggle();
    assert_eq!(writing, WritingStyle::SimplifiedTechnicalEnglish);
}
