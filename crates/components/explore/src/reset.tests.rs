use super::*;

#[test]
fn a_second_click_confirms_until_a_tick_cancels_the_wait() {
    let first = Instant::now();
    let mut reset = ResetConfirmation::default();

    assert!(!reset.confirms(first));
    // Past the deadline, but no tick cancelled the wait: the screen still shows Confirm reset.
    assert!(reset.confirms(first + Duration::from_secs(6)));
}
