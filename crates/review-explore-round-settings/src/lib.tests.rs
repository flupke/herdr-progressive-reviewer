use super::*;

#[test]
fn settings_saved_without_a_value_take_its_default() {
    let empty: ExploreRoundSettings = serde_json::from_str("{}").unwrap();

    assert_eq!(empty, ExploreRoundSettings::default());
}

#[test]
fn settings_read_back_as_they_were_saved() {
    for (writing, run_ahead, saved) in [
        (
            WritingStyle::Plain,
            RunAhead::Every,
            r#"{"writing":"plain","run_ahead":"every"}"#,
        ),
        (
            WritingStyle::SimplifiedTechnicalEnglish,
            RunAhead::Recommended,
            r#"{"writing":"simplified_technical_english","run_ahead":"recommended"}"#,
        ),
    ] {
        let settings = ExploreRoundSettings { writing, run_ahead };

        assert_eq!(serde_json::to_string(&settings).unwrap(), saved);
        assert_eq!(
            serde_json::from_str::<ExploreRoundSettings>(saved).unwrap(),
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

#[test]
fn run_ahead_is_off_by_default_and_in_settings_saved_before_it_existed() {
    let saved: ExploreRoundSettings = serde_json::from_str(r#"{"writing":"plain"}"#).unwrap();

    assert_eq!(saved.run_ahead, RunAhead::Off);
    assert_eq!(saved.writing, WritingStyle::Plain);
}

#[test]
fn run_ahead_cycles_through_the_recommended_choice_and_every_choice_back_to_off() {
    let mut run_ahead = RunAhead::Off;

    run_ahead.cycle();
    assert_eq!(run_ahead, RunAhead::Recommended);
    run_ahead.cycle();
    assert_eq!(run_ahead, RunAhead::Every);
    run_ahead.cycle();
    assert_eq!(run_ahead, RunAhead::Off);
}

#[test]
fn run_ahead_prepares_the_choices_its_value_names() {
    assert!(!RunAhead::Off.prepares(true));
    assert!(RunAhead::Recommended.prepares(true));
    assert!(!RunAhead::Recommended.prepares(false));
    assert!(RunAhead::Every.prepares(false));
    assert!(!RunAhead::Off.is_on());
    assert!(RunAhead::Recommended.is_on());
}
