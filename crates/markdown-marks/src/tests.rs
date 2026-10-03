use super::*;

#[test]
fn a_callout_marker_opens_the_text_in_any_case() {
    assert_eq!(
        Callout::strip("[!TIP] Run it twice."),
        Some((Callout::Tip, "Run it twice."))
    );
    assert_eq!(
        Callout::strip("  [!warning]\nThe cache is shared."),
        Some((Callout::Warning, "The cache is shared."))
    );
    assert_eq!(
        Callout::strip("[!CONCLUSION]"),
        Some((Callout::Conclusion, ""))
    );
}

#[test]
fn a_marker_counts_only_at_the_start_and_only_when_known() {
    assert_eq!(Callout::strip("See [!TIP] below."), None);
    assert_eq!(Callout::strip("[!NOTE] Unknown kind."), None);
    assert_eq!(Callout::strip("[!TI"), None);
    assert_eq!(StatusMark::strip("[!fine] 2 ms"), None);
    assert_eq!(StatusMark::strip("é"), None);
}

#[test]
fn a_status_mark_opens_a_cell() {
    assert_eq!(
        StatusMark::strip("[!good] 2 ms"),
        Some((StatusMark::Good, "2 ms"))
    );
    assert_eq!(StatusMark::strip("[!BAD]"), Some((StatusMark::Bad, "")));
    assert_eq!(
        StatusMark::strip("[!warning] slow on a cold cache"),
        Some((StatusMark::Warning, "slow on a cold cache"))
    );
}

/// Every marker of `M` is its own, and reads back as its mark.
fn markers_are_distinct<M: Mark + PartialEq + std::fmt::Debug>() {
    for (index, mark) in M::ALL.iter().enumerate() {
        let marker = mark.marker();
        assert!(
            M::ALL[index + 1..]
                .iter()
                .all(|other| other.marker() != marker),
            "{marker}"
        );
        assert_eq!(M::strip(marker), Some((*mark, "")));
    }
}

#[test]
fn every_mark_has_its_own_marker() {
    markers_are_distinct::<Callout>();
    markers_are_distinct::<StatusMark>();
}
