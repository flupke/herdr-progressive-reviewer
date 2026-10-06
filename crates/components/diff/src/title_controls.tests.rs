use super::TitleControls;
use crate::DiffControl;

#[test]
fn each_label_and_its_padding_select_its_control() {
    let controls = TitleControls::DIFF;
    // The title ends against the right corner of a 40-cell pane.
    let start = 40 - 1 - u16::try_from(controls.width()).unwrap();
    let at = |offset| controls.at(40, start + offset);

    assert_eq!(
        start
            .checked_sub(1)
            .and_then(|column| controls.at(40, column)),
        None
    );
    assert_eq!(at(0), Some(DiffControl::ExpandAll));
    assert_eq!(at(3), Some(DiffControl::ExpandAll));
    assert_eq!(at(4), Some(DiffControl::ContractAll));
    assert_eq!(at(8), Some(DiffControl::ShowFile));
    assert_eq!(at(11), Some(DiffControl::ShowFile));
    assert_eq!(at(12), None);
}

#[test]
fn a_pane_narrower_than_the_title_has_no_controls() {
    assert_eq!(TitleControls::DIFF.at(4, 1), None);
}
